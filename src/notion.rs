use crate::core::datatypes::{Block, Page};
use chrono::{Duration, Utc};
use log::debug;
use notion_client::{
    endpoints::{
        search::title::{
            request::{Filter, SearchByTitleRequestBuilder, Sort, SortDirection, Timestamp},
            response::PageOrDatabase,
        },
        Client,
    },
    objects::{
        block::{Block as NotionBlock, BlockType, ParagraphValue},
        page::{Page as NotionPage},
        rich_text::{RichText, Text, Annotations, TextColor},
    },
    NotionClientError,
};
use reqwest::ClientBuilder;
use std::collections::VecDeque;
use dendron::{Tree, Node};
use futures::future::{self, try_join_all};

pub struct Notion {
    client: Client,
}

impl Notion {
    pub fn new(token: String) -> Result<Self, NotionClientError> {
        let client = Client::new(token, Some(ClientBuilder::new()));
        match client {
            Ok(c) => Ok(Notion { client: c }),
            Err(e) => Err(e),
        }
    }

    pub async fn get_last_edited_pages(
        &self,
        dur: Duration,
    ) -> Result<Vec<Page>, NotionClientError> {

        let mut pages: Vec<Page> = Vec::new();
        let cutoff = Utc::now() - dur;
        let mut current_cursor: Option<String> = None;

        let mut req_builder = SearchByTitleRequestBuilder::default();
        req_builder.filter(Filter {
            value: notion_client::endpoints::search::title::request::FilterValue::Page,
            property: notion_client::endpoints::search::title::request::FilterProperty::Object,
        });
        req_builder.sort(Sort {
            timestamp: Timestamp::LastEditedTime,
            direction: SortDirection::Descending,
        });
        req_builder.page_size(100);

        loop {
            // paging
            if let Some(cursor) = current_cursor {
                req_builder.start_cursor(cursor);
            }

            // Send request
            // TODO might be able to use retrieve_page_property api here and get only last_edited, id, and title, which would
            // conserve bandwidth
            let res = self
                .client
                .search
                .search_by_title(req_builder.build().unwrap())
                .await?;

            current_cursor = res.next_cursor;
            let res_len = res.results.len();
            let mut current_notion_pages = res
                .results
                .into_iter()
                .filter_map(|page_or_db| match page_or_db {
                    PageOrDatabase::Page(page) => Some(page),
                    PageOrDatabase::Database(_) => None,
                })
                .collect::<Vec<NotionPage>>();
            if current_notion_pages.len() != res_len {
                // TODO improve error handling
                panic!("something other than a page was found in returned info. res_len: {res_len} currentpages.len(): {}", current_notion_pages.len());
            }

            // we only care about pages edited within `dur`, so we need to
            // cut out the Pages that were edited after `dur`
            let cutoff_index = current_notion_pages
                .iter()
                .position(|page| page.last_edited_time < cutoff);
            if let Some(index) = cutoff_index {
                current_notion_pages = current_notion_pages.split_at(index).0.to_vec();
            }

            pages.append(&mut try_join_all(current_notion_pages.into_iter().map(|notion_page| {
                self.from_notion_page(notion_page)
            })).await.unwrap());

            if !res.has_more || cutoff_index.is_some() {
                break;
            }
        }

        Ok(pages)
    }

    pub async fn get_page_root_blocks   (
        &self,
        page: &Page,
        dur: Duration,
    ) -> Option<Result<Vec<Block>, NotionClientError>> {

        debug!(target: "notion", "Page URL: {}", page.url);
        // TODO: figure out how to handle these with error handling rather than silently ignoring
        // these are special pages I use to hold hundreds of other child pages, and so it
        // takes forever to load. It doesn't contain any useful info, so skip it.
        if page.url.contains("Place-To-Store-Pages")
            || page.url.contains("Daily-Journal")
            || page.url.contains("Personal-")
            || page.url.contains("Roam-Import")
        {
            return  None;
        }
        Some(self.get_page_root_blocks_inner(page, dur).await)
    }

    async fn get_page_root_blocks_inner(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Result<Vec<Block>, NotionClientError> {
        let cutoff = Utc::now() - dur;
        let mut block_ids_to_process = VecDeque::new();
        let mut root_blocks: Vec<Block> = Vec::new();

        // simple inefficient solution right now: go through fetching all the
        // blocks that were edited with `dur`
        block_ids_to_process.push_back(page.id.clone());

        while let Some(block_id) = block_ids_to_process.pop_front() {
            let block_siblings = self.retrieve_all_notion_block_children(&block_id).await?;

            for block in block_siblings {
                if block.last_edited_time.unwrap() > cutoff {
                    // we don't recurse on its children, we'll process
                    // them later

                    let block_children_ids = if block.has_children.is_some_and(|b| b) {
                        self.retrieve_all_notion_block_children(&block.id.clone().unwrap())
                            .await?
                            .into_iter()
                            .map(|block| block.id.unwrap())
                            .collect()
                    } else {
                        Vec::new()
                    };
                    root_blocks.push(Block::from_notion_block(
                        block,
                        page.id.clone(),
                        block_children_ids,
                    ));
                } else {
                    // keep recursing down the tree of children blocks
                    block_ids_to_process.push_back(block.id.unwrap());
                }
            }
        }
        debug!(target: "notion", "fetched {} relevant but possibly-empty Blocks from Page {}", root_blocks.len(), page.url);
        debug!(target: "notion", "{:#?}", root_blocks);

        // filter out empty blocks
        let relevant_blocks: Vec<Block> = root_blocks
            .into_iter()
            .filter(|b| !b.is_empty())
            .collect();
        debug!(target: "notion", "fetched {} relevant Blocks from Page {}", relevant_blocks.len(), page.url);
        debug!(target: "notion", "{:#?}", relevant_blocks);

        Ok(relevant_blocks)

    }

    async fn retrieve_all_notion_block_children(
        &self,
        block_id: &str,
    ) -> Result<Vec<NotionBlock>, NotionClientError> {
        let mut children_blocks: Vec<NotionBlock> = Vec::new();
        let mut current_cursor: Option<String> = None;

        loop {
            let mut res = self
                .client
                .blocks
                .retrieve_block_children(block_id, current_cursor.as_deref(), Some(100))
                .await?;

            children_blocks.append(&mut res.results);

            if !res.has_more {
                break;
            }
            current_cursor = res.next_cursor.clone();
        }

        Ok(children_blocks)
    }

    async fn retrieve_all_block_children(
        &self,
        page_id: String,
        block_id: &str,
    ) -> Result<Vec<Block>, NotionClientError> {
        let mut blocks: Vec<Block> = Vec::new();

        let notion_blocks: Vec<NotionBlock> = self.retrieve_all_notion_block_children(block_id).await?.into_iter();
        for block in notion_blocks {
            let block_children_ids = if block.has_children.is_some_and(|b| b) {
                self.retrieve_all_notion_block_children(&block.id.clone().unwrap())
                    .await?
                    .into_iter()
                    .map(|block| block.id.unwrap())
                    .collect()
            } else {
                Vec::new()
            };
            blocks.push(Block::from_notion_block(block, page_id.clone(), block_children_ids));
        }
        
        Ok(blocks)
    }

    pub async fn page_and_blocks_to_tree(
        &self,
        (page, root_blocks): (Page, Vec<Block>)
    ) -> Result<Tree<Block>, NotionClientError> {
        
        // At last we have all of the page's children Blocks that were updated in the last `dur`
        // period of time and are non-empty. Now we will expand out these Blocks' children
        // recursively, and use that to write a markdown String that represents all of the
        // relevant Block content for this Page

        // let page_name = Notion::get_title_of_page(&page);
        // let mut page_name_line = "Page Name: ".to_string() + &page_name;
        // page_name_line.push('\n');
        // let mut page_markdown = page_name_line;

        // self.build_page_markdown(root_blocks, &mut page_markdown, 1).await?;

        // debug!("page_markdown:\n{}", page_markdown);

        // Ok(page_markdown)
    }

    async fn build_page_markdown(&self, blocks: Vec<Block>, page_markdown: &mut String, num_tabs: usize) -> Result<(), NotionClientError> {
        // TODO, figure out how to handle images
        if blocks.is_empty() {
            return Ok(());
        }
        for block in blocks {
            // add this Block's contribution to the Page's markdown string
            let mut line = "\t".repeat(num_tabs);
            line.push_str(&block.get_text());
            page_markdown.push_str(&line);
            page_markdown.push('\n');

            let block_children = self.retrieve_all_block_children(block.page_id, &block.id).await?;
            // note, we have the Box::pin so that we can call .await in a recursive function
            Box::pin(self.build_page_markdown(block_children, page_markdown, num_tabs + 1)).await?;
        }

        Ok(())
    }

    /// This is a hack to get the page name, but I don't know if it's very robust
    fn get_title_of_page(page: &Page) -> String {
        let url_fragment = page.url.split("/").last();
        match url_fragment {
            Some(name) => {
                let parts = name.split("-").collect::<Vec<&str>>();
                parts[..parts.len() - 1].join(" ")
            },
            None => "Unknown Page Title".to_string()
        }
    }

    async fn from_notion_page(&self, notion_page: NotionPage) -> Result<Page, NotionClientError> {
        Ok(Page {
            id: notion_page.id.clone(),
        url: notion_page.url.clone(),
        creation_date: notion_page.created_time,
        update_date: notion_page.last_edited_time,
        child_blocks: self.retrieve_all_block_children(notion_page.id.clone(), &notion_page.id).await?,
        })
    }
}