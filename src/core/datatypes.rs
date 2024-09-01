use chrono::{DateTime, Utc};
use log::debug;
use notion_client::objects::block::{Block as NotionBlock, BlockType};
use notion_client::objects::parent::Parent;
use notion_client::objects::rich_text::RichText;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub text: String,
    pub creation_date: DateTime<Utc>,
    pub update_date: DateTime<Utc>,
    pub parent_block_id: Option<String>,
    pub has_children: bool,
}

impl Block {
    pub fn from_notion_block(notion_block: NotionBlock, page_id: String) -> Self {
        Block {
            id: notion_block.id.unwrap_or_default(),
            // TODO: consider removing this, since it is stored multiple times
            // throughout all the blocks, and we don't need it specifically on a block
            // it's a nice-to-have right now
            page_id,
            // this is where the actual Block data is
            text: notion_block.block_type.plain_text()
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect::<Vec<String>>()
                .join(" "), // TODO: a space " " separator is not alwasy appropriate, but works for now. Find a better way to join the text
            creation_date: notion_block.created_time.unwrap_or_default(),
            update_date: notion_block.last_edited_time.unwrap_or_default(),
            parent_block_id: notion_block.parent.and_then(|parent| match parent {
                Parent::BlockId { block_id } => Some(block_id),
                _ => None,
            }),
            has_children: notion_block.has_children.unwrap_or_default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

}

pub struct Page {
    pub id: String,
    pub title: String,
    pub url: String,
    pub creation_date: DateTime<Utc>,
    pub update_date: DateTime<Utc>,
    pub child_blocks: Vec<Block>,
}
