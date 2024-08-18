use core::num;

use chrono::{DateTime, Utc};
use notion_client::objects::block::{self, Block as NotionBlock, BlockType};
use notion_client::objects::parent::Parent;
use notion_client::objects::rich_text::RichText;
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
    pub fn from_notion_block(
        notion_block: NotionBlock,
        page_id: String,
        child_block_ids: Vec<String>,
    ) -> Self {
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

    pub fn is_empty(&self) -> bool {
        match &self.block_type {
            BlockType::Paragraph { paragraph } => self.rich_text_is_empty(&paragraph.rich_text),
            BlockType::Heading1 { heading_1 } => self.rich_text_is_empty(&heading_1.rich_text),
            BlockType::Heading2 { heading_2 } => self.rich_text_is_empty(&heading_2.rich_text),
            BlockType::Heading3 { heading_3 } => self.rich_text_is_empty(&heading_3.rich_text),
            BlockType::BulletedListItem { bulleted_list_item } => {
                self.rich_text_is_empty(&bulleted_list_item.rich_text)
            }
            BlockType::NumberedListItem { numbered_list_item } => {
                self.rich_text_is_empty(&numbered_list_item.rich_text)
            }
            BlockType::Toggle { toggle } => self.rich_text_is_empty(&toggle.rich_text),
            // there might be some actually-empty blocks that we miss here by always
            // returning false, but it's better to err on the safe side for now
            _ => false,
        }
    }

    fn rich_text_is_empty(&self, rich_text: &Vec<RichText>) -> bool {
        rich_text.into_iter().all(|rt| match rt {
            RichText::Text { text, .. } => text.content.is_empty(),
            _ => true,
        })
    }
}
