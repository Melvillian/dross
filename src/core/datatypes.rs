use chrono::{DateTime, Utc};
use notion_client::objects::block::{self, Block as NotionBlock, BlockType};
use notion_client::objects::parent::Parent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: String,
    pub page_id: String,
    // for now we use notion_client's BlockTypes,
    // but when we expand to more notetaking sources
    // (e.g. Obsidian, Evernote, etc.) we'll need to
    // write our own BlockType that is not coupled
    // to notion_client's, because those notetaking
    // API's will have different block types than
    // notion's
    pub block_type: BlockType,
    pub creation_date: DateTime<Utc>,
    pub update_date: DateTime<Utc>,
    pub parent_block_id: Option<String>,
    pub child_block_ids: Vec<String>,
}

impl Block {
    pub fn from_notion_block(notion_block: NotionBlock, page_id: String, child_block_ids: Vec<String>) -> Self {
        Block {
            id: notion_block.id.unwrap_or_default(),
            page_id,
            // this is where the actual Block data is
            block_type: notion_block.block_type,
            creation_date: notion_block.created_time.unwrap_or_default(),
            update_date: notion_block.last_edited_time.unwrap_or_default(),
            parent_block_id: notion_block.parent.and_then(|parent| match parent {
                Parent::BlockId { block_id } => Some(block_id),
                _ => None,
            }),
            child_block_ids,
        }
    }
}
