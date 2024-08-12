use notion_client::{endpoints::{
    search::title::{request::{Filter, SearchByTitleRequestBuilder, Sort, SortDirection, Timestamp}, response::PageOrDatabase}, Client
}, objects::page::Page, NotionClientError};
use chrono::{Duration, Utc};
use crate::core::datatypes::Block;
use reqwest::ClientBuilder;

pub struct Notion {
    client: Client
}

impl Notion {
    pub fn new(token: String) -> Result<Self, NotionClientError> {
        let client = Client::new(token, Some(ClientBuilder::new()));
        match client {
            Ok(c) => Ok(Notion {
                client: c
            }),
            Err(e) => Err(e)
        }
    }

    pub async fn get_last_edited_pages(&self, dur: Duration) -> Result<Vec<Page>, NotionClientError> {
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
            let res = self.client
            .search
            .search_by_title(req_builder.build().unwrap())
            .await?;
    
            current_cursor = res.next_cursor;
            let res_len = res.results.len();
            let mut current_pages = res.results.into_iter().filter_map(
                |page_or_db| {
                    match page_or_db {
                        PageOrDatabase::Page(page) => Some(page),
                        PageOrDatabase::Database(_) => None,
                    }
                }
            ).collect::<Vec<Page>>();
            if current_pages.len() != res_len {
                // TODO improve error handling
                panic!("something other than a page was found in returned info");
            }
    
            // handle the case where a paginated response contains Pages older than `dur`
            let cutoff_index = current_pages.iter().position(|page| page.last_edited_time < cutoff);
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

pub async fn get_block_content_in_pages(&self, pages: Vec<Page>, dur: Duration) -> Result<Vec<Block>, NotionClientError> {

    let mut blocks: Vec<Block> = Vec::new();

    for page in pages {
        let mut block: Vec<Block> = self.recently_edited_blocks_for_page(&page, dur).await?;
        blocks.append(&mut block);
    }
    Ok(blocks)
}

    pub async fn recently_edited_blocks_for_page(&self, page: &Page, dur: Duration) -> Result<Vec<Block>, NotionClientError> {
        let mut blocks = Vec::new();
        let mut current_cursor: Option<String> = None;

        loop {
            let res = self.client
            .blocks
            .retrieve_block_children(&page.id, current_cursor.as_deref(), Some(100))
            .await?;

            // paging
            current_cursor = res.next_cursor;

            // there's no more pagination, time to break
            // note: this should extremely rarely, only for Notion Pages with 100+ blocks
            // under a single parent
            if !res.has_more {
                break;
            }
        }
        Ok(blocks)
    }
}
