use crate::core::datatypes::Block;
use chrono::{Duration, Utc};
use log::{debug, info};
use nary_tree::{NodeMut, Tree, TreeBuilder};
use notion_client::{
    endpoints::{
        search::title::{
            request::{Filter, SearchByTitleRequestBuilder, Sort, SortDirection, Timestamp},
            response::PageOrDatabase,
        },
        Client,
    },
    objects::{
        block::{self, Block as NotionBlock},
        page::Page,
    },
    NotionClientError,
};
use reqwest::ClientBuilder;
use std::{borrow::BorrowMut, rc::Rc};
use std::{cell::RefCell, collections::VecDeque};

pub struct Notion {
    client: Client,
}
use serde_json::json;

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
        info!(target: "notion", "fetching pages edited in the last {} days", dur.num_days());

        let mut pages = Vec::new();
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
                panic!("something other than a page was found in returned info. res_len: {res_len} currentpages.len(): {}", current_pages.len());
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
            debug!(target: "notion", "Page URL: {}", page.url);
            // TODO: figure out how to handle these with error handling rather than silently ignoring
            // these are special pages I use to hold hundreds of other child pages, and so it
            // takes forever to load. It doesn't contain any useful info, so skip it.
            if page.url.contains("Place-To-Store-Pages")
                || page.url.contains("Daily-Journal")
                || page.url.contains("Personal-")
                || page.url.contains("Roam-Import")
            {
                continue;
            }
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
                    relevant_blocks.push(Block::from_notion_block(
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
        debug!(target: "notion", "fetched {} relevant but possibly-empty Blocks from Page {}", relevant_blocks.len(), page.url);
        debug!(target: "notion", "{:#?}", relevant_blocks);

        // filter out empty blocks
        let relevant_blocks: Vec<Block> = relevant_blocks
            .into_iter()
            .filter(|b| !b.is_empty())
            .collect();
        debug!(target: "notion", "fetched {} relevant Blocks from Page {}", relevant_blocks.len(), page.url);
        debug!(target: "notion", "{:#?}", relevant_blocks);

        // At last we have all of the page's children Blocks that were updated in the last `dur`
        // period of time and are non-empty. Now we will expand out these Blocks' children
        // recursively, and use that to write a markdown String that represents all of the
        // relevant Block content for this Page

        // Page -> Block
        let page_notion_block = self.client.blocks.retrieve_a_block(&page.id).await?;
        let page_children_block_ids = self.retrieve_all_notion_block_children(&page.id).await?;
        let page_block = Block::from_notion_block(
            page_notion_block,
            page.id.clone(),
            page_children_block_ids
                .into_iter()
                .map(|block| block.id.unwrap())
                .collect(),
        );

        // the Page these blocks comes from is always the root of the tree, and the nested
        // Block children are children of the tree root and siblings of each other
        // (regardless of how nested they were in the original Notion Page)

        let mut tree = TreeBuilder::new().with_root(page_block).build();
        let tree_root = Rc::new(RefCell::new(tree.root_mut().unwrap()));
        let mut blocks_to_add_to_tree: VecDeque<BlockAndParentTreeNode> = VecDeque::from_iter(
            relevant_blocks
                .into_iter()
                .map(|b| BlockAndParentTreeNode::new(b, tree_root.clone())),
        );

        while let Some(BlockAndParentTreeNode { block, parent_node }) =
            blocks_to_add_to_tree.pop_front()
        {
            let mut parent_node = parent_node.borrow_mut();
            let this_block_node = parent_node.append(block.clone());

            let block_children = if !block.child_block_ids.is_empty() {
                self.retrieve_all_block_children(block.page_id, &block.id).await?
            } else {
                Vec::new()
            };

            blocks_to_add_to_tree.extend(
                block_children
                    .into_iter()
                    .map(|b: Block| BlockAndParentTreeNode::new(b, Rc::new(RefCell::new(this_block_node.clone())))),
            );
        }
        Ok(tree)
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

        let notion_blocks: Vec<NotionBlock> = self.retrieve_all_notion_block_children(block_id).await?;
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
}

struct BlockAndParentTreeNode<'a> {
    block: Block,
    parent_node: Rc<RefCell<NodeMut<'a, Block>>>,
}

impl BlockAndParentTreeNode<'_> {
    fn new<'a>(
        block: Block,
        parent_node: Rc<RefCell<NodeMut<'a, Block>>>,
    ) -> BlockAndParentTreeNode<'a> {
        BlockAndParentTreeNode { block, parent_node }
    }
}
