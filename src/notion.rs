use crate::core::datatypes::{Block, Page};
use chrono::{Duration, Utc};
use dendron::{Node, Tree};
use log::{debug, error, trace};
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
use std::collections::{HashSet, VecDeque};

pub struct Notion {
    client: Client,
}

impl Notion {
    pub fn new(token: String) -> Result<Self, NotionClientError> {
        let client = Client::new(token, None);
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
        req_builder
            .filter(Filter {
                value: notion_client::endpoints::search::title::request::FilterValue::Page,
                property: notion_client::endpoints::search::title::request::FilterProperty::Object,
            })
            .sort(Sort {
                timestamp: Timestamp::LastEditedTime,
                direction: SortDirection::Descending,
            })
            .page_size(100);

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
                let page = self.notion_page_to_dross_page(notion_page).await?;
                pages.push(page);
            }

            if !res.has_more || cutoff_index.is_some() {
                break;
            }
        }

        Ok(pages)
    }

    /// For a given Notion `Page`, retrieve all of its non-empty children, grandchildren, etc... `Block`s that were edited within the specified duration.
    ///
    /// Uses breadth-first-search to recursively fetch all the block descendants of the page.
    ///
    /// # Returns
    /// A `Result` containing a `Vec` of all the `Page`'s descentant `Block`s that were updated between within `dur`. Note
    /// that this includes the `Page` `Block` itself. Also note that the order of the `Block`s is not guaranteed and
    /// cannot be relied upon.
    pub async fn get_page_block_roots(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Result<Vec<Block>, NotionClientError> {
        let cutoff = Utc::now() - dur;
        let mut block_ids_to_process = VecDeque::new();
        let mut block_roots: Vec<Block> = Vec::new();
        let mut already_visited: HashSet<String> = HashSet::new();

        // some user's Pages are huuuge, so long that we don't know if we'll spend too much time
        // much time fetching all their children. So, as a heuristic for when to abort we use
        // a fixed time (time_to_spend_fetching_children) after which we abort and use whichever
        // block roots (if any) we have
        let time_to_spend_fetching_children = Duration::seconds(30);
        let abort_time = Utc::now() + time_to_spend_fetching_children;

        block_ids_to_process.push_back(page.id.clone());

        while let Some(block_id) = block_ids_to_process.pop_front() {
            if already_visited.contains(&block_id) {
                trace!(
                    target: "notion",
                    "already visited this block {}, skipping it...",
                    &block_id
                );
                // we've already processed this block, so skip it
                continue;
            }
            trace!(
                target: "notion",
                "getting block root with id {}",
                &block_id
            );
            let children = self
                .retrieve_all_block_children(&page.id, &block_id)
                .await?;

            for block in children {
                if block.update_date >= cutoff {
                    // is the Block's edit time within the duration?
                    if !block.is_empty() {
                        // note, there may be further descendants of this block that were
                        // edited within the duration, but we will process those in a later
                        // function
                        block_roots.push(block);
                    }
                } else {
                    // keep recursing down the tree of children blocks
                    block_ids_to_process.push_back(block.id);
                }
            }

            already_visited.insert(block_id);

            if Utc::now() > abort_time {
                // we've spent too much time fetching children, so just return what we have
                debug!(target: "notion", "aborting block retrieval due to time limit");
                debug!(target: "notion", "returning {} block roots for page: {}", block_roots.len(), page.title);
                break;
            }
        }

        debug!(target: "notion", "fetched {} descendant Blocks from Page {}", block_roots.len(), page.url);
        debug!(target: "notion", "{:#?}", block_roots);

        Ok(block_roots)
    }

    pub async fn retrieve_all_block_children(
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

            let mut there_was_an_error = false;
            let res: RetrieveBlockChilerenResponse = match res {
                Ok(res) => res,
                Err(e) => match e {
                    NotionClientError::FailedToDeserialize { source: _, body } => {
                        there_was_an_error = true;
                        let json_value: serde_json::Value = serde_json::from_str(&body).unwrap();
                        let pretty_json = serde_json::to_string_pretty(&json_value).unwrap();
                        debug!(target: "notion", "Custom Failed to deserialize response body");
                        debug!(target: "notion", "{}", pretty_json);
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

            if there_was_an_error {
                debug!(target: "notion", "there was an error but we made it past so we must have block children {:?}", res.results);
            }

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

    /// Given a `Vec` of `Block`s (call these `Block`s "roots") that have been updated recently,
    /// return a `Tree`-like representation of each each root and its descendants by recursively
    /// fetching the children of each root, and the children of those children, etc...
    ///
    /// The goal here is to create a tree structure that mimics of nested structure of a page
    /// notes, where the nesting is achieved by indenting the text of each block under its parent.
    ///
    /// So we want to go from a `Vec` of `Block`s like:
    ///
    ///      block_root_1         block_root_2     ....    block_root_n
    ///
    /// and end with something that, represented in tree-fashion, looks like:
    ///
    ///      block_root_1         block_root_2     ....    block_root_n
    ///          |                     |                       |
    ///    +-----+-----+         +-----+-----+       .        ZZZ
    ///    |     |     |         |           |       .
    ///   A      B     C         J           K       .
    ///   |      |     |         |           |       .
    ///  +-+    +-+   +-+       +---+       +-+      .
    ///  | |    | |   | |       | | |       | |      .
    ///  D E    F G   H I       L M N       O P      .
    ///
    pub async fn grow_the_roots(
        &self,
        block_roots: Vec<Block>,
    ) -> Result<Vec<Tree<Block>>, NotionClientError> {
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
