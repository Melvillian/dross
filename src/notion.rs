use crate::core::datatypes::Block;
use chrono::{Duration, Utc};
use nary_tree::Tree;
use notion_client::{
    endpoints::{
        search::title::{
            request::{Filter, SearchByTitleRequestBuilder, Sort, SortDirection, Timestamp},
            response::PageOrDatabase,
        },
        Client,
    },
    objects::block::Block as NotionBlock,
    objects::page::Page,
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
        let mut pages = Vec::new();
        let cutoff = Utc::now() - dur;
        let mut current_cursor: Option<String> = None;

        // Set up request parameters
        // TODO, cache this
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
            let mut current_pages = res
                .results
                .into_iter()
                .filter_map(|page_or_db| match page_or_db {
                    PageOrDatabase::Page(page) => Some(page),
                    PageOrDatabase::Database(_) => None,
                })
                .collect::<Vec<Page>>();
            if current_pages.len() != res_len {
                // TODO improve error handling
                panic!("something other than a page was found in returned info");
            }

            // handle the case where a paginated response contains Pages older than `dur`
            let cutoff_index = current_pages
                .iter()
                .position(|page| page.last_edited_time < cutoff);
            if let Some(index) = cutoff_index {
                current_pages = current_pages.split_at(index).0.to_vec();
                pages.append(&mut current_pages);
                break;
            }

            pages.append(&mut current_pages);

            // there's no more pages, time to break
            // note: this should extremely rarely, only for Notion integrations less than `dur` old
            if !res.has_more {
                break;
            }
        }

        Ok(pages)
    }

    pub async fn pages_to_blocks(
        &self,
        pages: Vec<Page>,
        dur: Duration,
    ) -> Result<Vec<Tree<Block>>, NotionClientError> {
        let mut blocks: Vec<Tree<Block>> = Vec::new();

        for page in pages {
            let page_of_blocks: Tree<Block> = self.page_blocks(&page, dur).await?;
            blocks.push(page_of_blocks);
        }
        Ok(blocks)
    }

    pub async fn page_blocks(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Result<Tree<Block>, NotionClientError> {
        let cutoff = Utc::now() - dur;
        let mut block_ids_to_process = VecDeque::new();
        let mut relevant_blocks: Vec<Block> = Vec::new();

        // simple inefficient solution right now: go through fetching all the
        // blocks that were edited with `dur`, and then from their build up the Vec<Block> using
        // the contents of those NotionBlocks
        block_ids_to_process.push_front(page.id.clone());

        while let Some(block_id) = block_ids_to_process.pop_back() {
            let block_siblings = self.retrieve_all_block_children(&block_id).await?;

            for block in block_siblings {
                let block_is_within_duration = block.last_edited_time.unwrap() > cutoff;
                if block_is_within_duration {
                    // we don't recurse on its children, we'll process
                    // them later

                    let block_children_ids = self
                        .retrieve_all_block_children(&block.id.clone().unwrap())
                        .await?
                        .into_iter()
                        .map(|block| block.id.unwrap())
                        .collect();
                    relevant_blocks.push(Block::from_notion_block(block, block_children_ids));
                } else {
                    // keep recursing down the tree of children blocks
                    block_ids_to_process.push_front(block.id.unwrap());
                }
            }
        }

        // now we have all of the page's blocks that were updated in the last `dur`
        // period of time. Now we will convert them to our core::Block representation,
        // and store them in an n-ary tree, where NotionBlocks that were
        // parents of NotionBlocks will be parents of the Blocks in this Tree

        println!("relevant_blocks:\n{:#?}", relevant_blocks);

        let tree = Tree::new();

        for notion_block in relevant_blocks {}
        Ok(tree)
    }

    async fn retrieve_all_block_children(
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
}
