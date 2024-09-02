use crate::core::datatypes::{Block, Page};
use chrono::{Duration, Utc};
use dendron::{Node, Tree};
use log::{debug, error};
use notion_client::{
    endpoints::{
        blocks::retrieve::response::RetrieveBlockChilerenResponse,
        search::title::{
            request::{Filter, SearchByTitleRequestBuilder, Sort, SortDirection, Timestamp},
            response::PageOrDatabase,
        },
        Client,
    },
    objects::page::Page as NotionPage,
    NotionClientError,
};
use reqwest::ClientBuilder;
use std::collections::VecDeque;

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

            for notion_page in current_notion_pages {
                debug!("made it!");
                let page = self.notion_page_to_dross_page(notion_page).await?;
                pages.push(page);
            }

            // pages.append(
            //     &mut try_join_all(
            //         current_notion_pages
            //             .into_iter()
            //             .map(|notion_page| self.from_notion_page(notion_page)),
            //     )
            //     .await
            //     .unwrap(),
            // );

            if !res.has_more || cutoff_index.is_some() {
                break;
            }
        }

        Ok(pages)
    }

    pub async fn get_page_block_roots(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Option<Result<Vec<Block>, NotionClientError>> {
        debug!(target: "notion", "Page URL: {}", page.url);
        // TODO: figure out how to handle these with error handling rather than silently ignoring
        // these are special pages I use to hold hundreds of other child pages, and so it
        // takes forever to load. It doesn't contain any useful info, so skip it.
        if page.url.contains("Place-To-Store-Pages-")
            || page.url.contains("Daily-Journal-")
            || page.url.contains("Personal-")
            || page.url.contains("Roam-Import-")
        {
            return None;
        }
        Some(self.get_page_block_roots_inner(page, dur).await)
    }

    async fn get_page_block_roots_inner(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Result<Vec<Block>, NotionClientError> {
        let cutoff = Utc::now() - dur;
        let mut block_ids_to_process = VecDeque::new();
        let mut block_roots: Vec<Block> = Vec::new();

        // simple inefficient solution right now: go through fetching all the
        // blocks that were edited with `dur`
        block_ids_to_process.push_back(page.id.clone());

        while let Some(block_id) = block_ids_to_process.pop_front() {
            let block_siblings = self
                .retrieve_all_block_children(&page.id, &block_id)
                .await?;

            for block in block_siblings {
                if block.update_date > cutoff {
                    // we don't recurse on its children, we'll process
                    // them later
                    block_roots.push(block);
                } else {
                    // keep recursing down the tree of children blocks
                    block_ids_to_process.push_back(block.id);
                }
            }
        }
        debug!(target: "notion", "fetched {} relevant but possibly-empty Blocks from Page {}", block_roots.len(), page.url);
        debug!(target: "notion", "{:#?}", block_roots);

        // filter out empty blocks
        let relevant_blocks: Vec<Block> =
            block_roots.into_iter().filter(|b| !b.is_empty()).collect();
        debug!(target: "notion", "fetched {} relevant Blocks from Page {}", relevant_blocks.len(), page.url);
        debug!(target: "notion", "{:#?}", relevant_blocks);

        Ok(relevant_blocks)
    }

    async fn retrieve_all_block_children(
        &self,
        page_id: &str,
        block_id: &str,
    ) -> Result<Vec<Block>, NotionClientError> {
        let mut children_blocks: Vec<Block> = Vec::new();
        let mut current_cursor: Option<String> = None;

        loop {
            let res = self
                .client
                .blocks
                .retrieve_block_children(block_id, current_cursor.as_deref(), Some(100))
                .await;

            let res: RetrieveBlockChilerenResponse = match res {
                Ok(res) => res,
                Err(e) => match e {
                    NotionClientError::FailedToDeserialize { source: _, body } => {
                        debug!(target: "notion", "Custom Failed to deserialize response body: {body}");
                        // there seems to be some bug in notion-client where it's unable to handle these
                        // Response bodies, so I need to manually deserialize them here
                        // TODO research further what's going on here
                        serde_json::from_str(&body).unwrap()
                    }
                    _ => {
                        error!(target: "notion", "Custom error in retrieve_block_children {}", e);
                        panic!("{}", e)
                    }
                },
            };

            children_blocks.append(
                &mut res
                    .results
                    .into_iter()
                    .map(|block| Block::from_notion_block(block, page_id.to_string()))
                    .collect(),
            );

            if !res.has_more {
                break;
            }
            current_cursor = res.next_cursor.clone();
        }

        Ok(children_blocks)
    }

    pub async fn grow_the_roots(
        &self,
        block_roots: Vec<Block>,
    ) -> Result<Vec<Tree<Block>>, NotionClientError> {
        // At last we have all of the page's children Blocks that were updated in the last `dur`
        // period of time and are non-empty. Now we will expand out these Blocks' children
        // recursively, and use that to create a tree of each Page's structure
        let mut blossomed_roots = Vec::new();
        for block in block_roots {
            let root = Node::new_tree(block);
            blossomed_roots.push(root.tree());
            let mut queue = VecDeque::new();

            queue.push_back(root);
            while let Some(node) = queue.pop_front() {
                let grant = node.tree().grant_hierarchy_edit().unwrap();

                // TODO: figure out how to make this more efficient by not copying every block value
                let page_id = node.borrow_data().page_id.clone();
                let block_id = node.borrow_data().id.clone();
                let has_children = node.borrow_data().has_children;

                if has_children {
                    let children = self
                        .retrieve_all_block_children(&page_id, &block_id)
                        .await?;
                    for child in children {
                        node.create_as_last_child(&grant, child);
                        queue.push_back(node.last_child().unwrap());
                    }
                }
            }
        }

        Ok(blossomed_roots)
    }

    // async fn build_page_markdown(
    //     &self,
    //     blocks: Vec<Block>,
    //     page_markdown: &mut String,
    //     num_tabs: usize,
    // ) -> Result<(), NotionClientError> {
    //     // TODO, figure out how to handle images
    //     if blocks.is_empty() {
    //         return Ok(());
    //     }
    //     for block in blocks {
    //         // add this Block's contribution to the Page's markdown string
    //         let mut line = "\t".repeat(num_tabs);
    //         line.push_str(&block.get_text());
    //         page_markdown.push_str(&line);
    //         page_markdown.push('\n');

    //         let block_children = self
    //             .retrieve_all_block_children(&block.page_id, &block.id)
    //             .await?;
    //         // note, we have the Box::pin so that we can call .await in a recursive function
    //         Box::pin(self.build_page_markdown(block_children, page_markdown, num_tabs + 1)).await?;
    //     }

    //     Ok(())
    // }

    async fn notion_page_to_dross_page(
        &self,
        notion_page: NotionPage,
    ) -> Result<Page, NotionClientError> {
        Ok(Page {
            id: notion_page.id.clone(),
            // convert https://www.notion.so/August-19-2024-651d530e07a14f9c97b4084614c5049b -> August 19 2024
            // Note: yes, this is kinda hacky and won't work for every page title, but it's good enough
            // for getting the gist of what the page is called
            title: match notion_page.url.split("/").last() {
                Some(name) => {
                    let parts = name.split("-").collect::<Vec<&str>>();
                    parts.split_at(parts.len() - 1).0.join(" ")
                }
                None => "Unknown Page Title".to_string(),
            },
            url: notion_page.url.clone(),
            creation_date: notion_page.created_time,
            update_date: notion_page.last_edited_time,
            child_blocks: self
                .retrieve_all_block_children(&notion_page.id, &notion_page.id)
                .await?,
        })
    }
}
