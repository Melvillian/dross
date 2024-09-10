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
    objects::{block::Block as NotionBlock, page::Page as NotionPage},
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

    pub async fn get_page_block_roots(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Option<Result<Vec<Block>, NotionClientError>> {
        debug!(target: "notion", "Page URL: {}", page.url);
        // TODO: figure out how to handle these with error handling rather than silently ignoring
        // these are special pages I use to hold hundreds of other child pages, and so it
        // takes forever to load. It doesn't contain any useful info, so skip it.
        // if page.url.contains("Place-To-Store-Pages-")
        //     || page.url.contains("Daily-Journal-")
        //     || page.url.contains("Personal-")
        //     || page.url.contains("Roam-Import-")
        // {
        //     return None;
        // }
        Some(self.get_page_block_roots_inner(page, dur).await)
    }

    /// For a given Notion `Page`, retrieve all of its non-empty children, grandchildren, etc... `Block`s that were edited within the specified duration.
    ///
    /// Uses breadth-first-search to recursively fetch all the block descendants of the page.
    ///
    /// # Returns
    /// A `Result` containing a `Vec` of all the `Page`'s descentant `Block`s that were updated between within `dur`. Note
    /// that this includes the `Page` `Block` itself. Also note that the order of the `Block`s is not guaranteed and
    /// cannot be relied upon.
    async fn get_page_block_roots_inner(
        &self,
        page: &Page,
        dur: Duration,
    ) -> Result<Vec<Block>, NotionClientError> {
        let cutoff = Utc::now() - dur;
        let mut block_ids_to_process = VecDeque::new();
        let mut block_roots: Vec<Block> = Vec::new();
        let mut already_visited: HashSet<String> = HashSet::new();

        block_ids_to_process.push_back(page.id.clone());

        while let Some(block_id) = block_ids_to_process.pop_front() {
            if already_visited.contains(&block_id) {
                // we've already processed this block, so skip it
                continue;
            }
            let block_siblings = self
                .retrieve_all_block_children(&page.id, &block_id)
                .await?;

            for block in block_siblings {
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
        }

        debug!(target: "notion", "fetched {} descendant Blocks from Page {}", block_roots.len(), page.url);
        debug!(target: "notion", "{:#?}", block_roots);

        Ok(block_roots)
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

#[cfg(test)]
mod tests {
    use super::*;

    const PRETTY_JSON: &str = r#"
[
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T10:53:00.000Z",
      "has_children": false,
      "id": "35b1ec3c-269c-46f2-b5fb-5a87380f75f3",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T10:53:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": [
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.veic.org/careers",
            "plain_text": "https://www.veic.org/careers",
            "text": {
              "content": "https://www.veic.org/careers",
              "link": {
                "url": "https://www.veic.org/careers"
              }
            },
            "type": "text"
          }
        ]
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T10:53:00.000Z",
      "has_children": false,
      "id": "e75155bb-630f-4924-a267-06b566a26f66",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T10:53:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T10:53:00.000Z",
      "has_children": false,
      "id": "63789709-c486-4ec8-b1ef-bc48744a5458",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T10:56:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": [
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": "Great read by ",
            "text": {
              "content": "Great read by ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/ba84a70478ef4cfab72cf1204235d3f6",
            "mention": {
              "page": {
                "id": "ba84a704-78ef-4cfa-b72c-f1204235d3f6"
              },
              "type": "page"
            },
            "plain_text": "IEEE",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " about ",
            "text": {
              "content": " about ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/0c551cdb7b3b4c90a68915ad280a5225",
            "mention": {
              "page": {
                "id": "0c551cdb-7b3b-4c90-a689-15ad280a5225"
              },
              "type": "page"
            },
            "plain_text": "ultrasound",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " ",
            "text": {
              "content": " ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://spectrum.ieee.org/mems-ultrasound-history",
            "plain_text": "https://spectrum.ieee.org/mems-ultrasound-history",
            "text": {
              "content": "https://spectrum.ieee.org/mems-ultrasound-history",
              "link": {
                "url": "https://spectrum.ieee.org/mems-ultrasound-history"
              }
            },
            "type": "text"
          }
        ]
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T13:16:00.000Z",
      "has_children": false,
      "id": "72ea56bd-a28d-44e5-a0d4-3ecfae4aa63b",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T13:16:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T10:56:00.000Z",
      "has_children": false,
      "id": "b72b854c-9407-403f-ace1-9f176ba5d8b1",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T13:16:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": [
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": "Categories for ",
            "text": {
              "content": "Categories for ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/967b78003adb4672861c35bbb944394d",
            "mention": {
              "page": {
                "id": "967b7800-3adb-4672-861c-35bbb944394d"
              },
              "type": "page"
            },
            "plain_text": "srs",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " from ",
            "text": {
              "content": " from ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/0e40b9e798ab4a4a9e652b4ed3223f25",
            "mention": {
              "page": {
                "id": "0e40b9e7-98ab-4a4a-9e65-2b4ed3223f25"
              },
              "type": "page"
            },
            "plain_text": "brainscape",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " ",
            "text": {
              "content": " ",
              "link": null
            },
            "type": "text"
          }
        ]
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T13:16:00.000Z",
      "has_children": false,
      "id": "9686a5e4-6d98-4c8b-a43c-ea9cbb809122",
      "image": {
        "caption": [],
        "file": {
          "expiry_time": "2024-09-12T16:32:34.741Z",
          "url": "https://prod-files-secure.s3.us-west-2.amazonaws.com/50993cf9-ce82-41d8-9512-c7fb53c7d2ee/f5bf28a3-2158-4fb1-97e6-a8d837083bf7/Untitled.png?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Content-Sha256=UNSIGNED-PAYLOAD&X-Amz-Credential=AKIAT73L2G45HZZMZUHI%2F20240912%2Fus-west-2%2Fs3%2Faws4_request&X-Amz-Date=20240912T153234Z&X-Amz-Expires=3600&X-Amz-Signature=47781ca45062ea8e198ebd75e672b0477b3a9db00991d087bc3542e4ef0ee7bb&X-Amz-SignedHeaders=host&x-id=GetObject"
        },
        "type": "file"
      },
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T13:16:00.000Z",
      "object": "block",
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "image"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T14:04:00.000Z",
      "has_children": false,
      "id": "e308012b-4d6b-4747-a0f7-97e97d637cd4",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T14:04:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T14:04:00.000Z",
      "has_children": true,
      "id": "2f891011-8103-4a9a-9fb6-d95387d29575",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T16:46:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": [
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": "Talks with ",
            "text": {
              "content": "Talks with ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/0ed0409412234d84af888fc54e8d4cbf",
            "mention": {
              "page": {
                "id": "0ed04094-1223-4d84-af88-8fc54e8d4cbf"
              },
              "type": "page"
            },
            "plain_text": "Will Clausen",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " about ",
            "text": {
              "content": " about ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/967b78003adb4672861c35bbb944394d",
            "mention": {
              "page": {
                "id": "967b7800-3adb-4672-861c-35bbb944394d"
              },
              "type": "page"
            },
            "plain_text": "srs",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " ",
            "text": {
              "content": " ",
              "link": null
            },
            "type": "text"
          }
        ]
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-30T16:20:00.000Z",
      "has_children": true,
      "id": "3afef8a5-6870-453c-95b0-4f90c48b77ad",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-30T16:20:00.000Z",
      "object": "block",
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "synced_block": {
        "synced_from": null
      },
      "type": "synced_block"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-30T16:19:00.000Z",
      "has_children": false,
      "id": "c4f8a598-08ca-4a4b-bf02-88f698afe7ec",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-30T16:19:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T17:33:00.000Z",
      "has_children": true,
      "id": "4c1046c3-c0fa-40de-b7eb-a4321c86ad49",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T20:21:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": [
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": "thoughts with ",
            "text": {
              "content": "thoughts with ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/e436b047f02e46c7a52ad40e52201c7e",
            "mention": {
              "page": {
                "id": "e436b047-f02e-46c7-a52a-d40e52201c7e"
              },
              "type": "page"
            },
            "plain_text": "Sean Bjornsson",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " ",
            "text": {
              "content": " ",
              "link": null
            },
            "type": "text"
          }
        ]
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T20:21:00.000Z",
      "has_children": false,
      "id": "1a0e16d2-ce5d-4d38-a618-5f4930d7e5ce",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T20:21:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T20:21:00.000Z",
      "has_children": false,
      "id": "c28af59e-bfc4-4796-b31b-ba8aa92e1f7f",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T20:21:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": [
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": "Great ",
            "text": {
              "content": "Great ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/99d92598988040f285986f03d466ef7c",
            "mention": {
              "page": {
                "id": "99d92598-9880-40f2-8598-6f03d466ef7c"
              },
              "type": "page"
            },
            "plain_text": "blog",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " about how the ",
            "text": {
              "content": " about how the ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/de2ae242d8ca4ed09a06eedc89b5c18c",
            "mention": {
              "page": {
                "id": "de2ae242-d8ca-4ed0-9a06-eedc89b5c18c"
              },
              "type": "page"
            },
            "plain_text": "internet",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " and ",
            "text": {
              "content": " and ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://www.notion.so/c6e76f26a9e14e2d83bf879451452923",
            "mention": {
              "page": {
                "id": "c6e76f26-a9e1-4e2d-83bf-879451452923"
              },
              "type": "page"
            },
            "plain_text": "CS / Programming",
            "type": "mention"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": null,
            "plain_text": " works: ",
            "text": {
              "content": " works: ",
              "link": null
            },
            "type": "text"
          },
          {
            "annotations": {
              "bold": false,
              "code": false,
              "color": "default",
              "italic": false,
              "strikethrough": false,
              "underline": false
            },
            "href": "https://cs.fyi/guide/how-does-internet-work",
            "plain_text": "https://cs.fyi/guide/how-does-internet-work",
            "text": {
              "content": "https://cs.fyi/guide/how-does-internet-work",
              "link": {
                "url": "https://cs.fyi/guide/how-does-internet-work"
              }
            },
            "type": "text"
          }
        ]
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T20:21:00.000Z",
      "has_children": false,
      "id": "544f3497-90d9-44af-a426-229d84915fa5",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T20:21:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    },
    {
      "archived": false,
      "created_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "created_time": "2024-03-22T20:21:00.000Z",
      "has_children": false,
      "id": "67da5a03-557c-41f8-a143-ea90eb00f300",
      "in_trash": false,
      "last_edited_by": {
        "id": "5be127e8-c6d7-4a7b-a46d-a0eb3bc9d6af",
        "object": "user"
      },
      "last_edited_time": "2024-03-22T20:21:00.000Z",
      "object": "block",
      "paragraph": {
        "color": "default",
        "rich_text": []
      },
      "parent": {
        "page_id": "c870852e-d52d-4eb0-8618-a143adcad389",
        "type": "page_id"
      },
      "type": "paragraph"
    }
  ]
"#;

    #[test]
    fn test_notion_client_new() {
        let blocks: Vec<NotionBlock> = serde_json::from_str(PRETTY_JSON).unwrap();
    }
}
