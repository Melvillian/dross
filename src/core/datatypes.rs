use chrono::{DateTime, Utc};
use notion_client::objects::block::{Block as NotionBlock, BlockType};
use notion_client::objects::parent::Parent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub block_type: BlockType,
    pub text: String,
    pub creation_date: DateTime<Utc>,
    pub update_date: DateTime<Utc>,
    pub parent_block_id: Option<String>,
    pub has_children: bool,
}

impl Block {
    #[must_use]
    pub fn from_notion_block(notion_block: NotionBlock, page_id: String) -> Self {
        Block {
            id: notion_block.id.unwrap_or_default(),
            // TODO: consider removing this, since it is stored multiple times
            // throughout all the blocks, and we don't need it specifically on a block
            // it's a nice-to-have right now
            page_id,
            block_type: notion_block.block_type.clone(),
            // this is where the actual Block data is
            text: notion_block
                .block_type
                // TODO: notion-client mushes all of the text of certain BlockTypes (NumberedListItem, BulletListItem, Toggle, ToDo,
                // maybe some others) into a single Vec<Option<String>>, which is not great. When there's a need we should go back here
                // and do our own, more markdown-friendly way of extractin text for the different BlockTypes
                .plain_text()
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect::<Vec<String>>()
                .join(" "), // TODO: a space " " separator is not always appropriate, but works for now. Find a better way to join the text
            creation_date: notion_block.created_time.unwrap_or_default(),
            update_date: notion_block.last_edited_time.unwrap_or_default(),
            parent_block_id: notion_block.parent.and_then(|parent| match parent {
                Parent::BlockId { block_id } => Some(block_id),
                _ => None,
            }),
            has_children: notion_block.has_children.unwrap_or_default(),
        }
    }

    #[must_use]
    pub fn to_markdown(&self) -> String {
        match &self.block_type {
            BlockType::Heading1 { heading_1: _ } => format!("# {}", self.text),
            BlockType::Heading2 { heading_2: _ } => format!("## {}", self.text),
            BlockType::Heading3 { heading_3: _ } => format!("### {}", self.text),
            BlockType::BulletedListItem {
                bulleted_list_item: _,
            } => format!("- {}", self.text),
            BlockType::NumberedListItem {
                numbered_list_item: _,
            } => format!("1. {}", self.text),
            BlockType::ToDo { to_do: _ } => format!("- [ ] {}", self.text),
            BlockType::Toggle { toggle: _ } => format!("> {}", self.text),
            _ => format!("{}", self.text),
        }
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub url: String,
    pub creation_date: DateTime<Utc>,
    pub update_date: DateTime<Utc>,
    pub child_blocks: Vec<Block>,
}

#[cfg(test)]
mod tests {
    use notion_client::objects::{
        block::{BulletedListItemValue, TextColor},
        rich_text::{RichText, Text},
    };

    use super::*;

    #[test]
    fn test_block_to_markdown() {
        let blocks = vec![
            Block {
                id: "1".to_string(),
                block_type: BlockType::Heading1 {
                    heading_1: Default::default(),
                },
                text: "Heading 1".to_string(),
                creation_date: Utc::now(),
                update_date: Utc::now(),
                parent_block_id: None,
                has_children: false,
                page_id: "7b1b3b0c-14cb-45a6-a4b6-d2b48faecccb".to_string(),
            },
            Block {
                id: "2".to_string(),
                block_type: BlockType::Heading2 {
                    heading_2: Default::default(),
                },
                text: "Heading 2".to_string(),
                creation_date: Utc::now(),
                update_date: Utc::now(),
                parent_block_id: None,
                has_children: false,
                page_id: "7b1b3b0c-14cb-45a6-a4b6-d2b48faecccb".to_string(),
            },
            Block {
                id: "3".to_string(),
                block_type: BlockType::BulletedListItem {
                    bulleted_list_item: BulletedListItemValue {
                        rich_text: vec![RichText::Text {
                            plain_text: Some("Bullet point".to_string()),
                            href: None,
                            annotations: None,
                            text: Text {
                                content: "Bullet point".to_string(),
                                link: None,
                            },
                        }],
                        color: TextColor::Default,
                        children: None,
                    },
                },
                text: "Bullet point".to_string(),
                creation_date: Utc::now(),
                update_date: Utc::now(),
                parent_block_id: None,
                has_children: false,
                page_id: "7b1b3b0c-14cb-45a6-a4b6-d2b48faecccb".to_string(),
            },
            Block {
                id: "4".to_string(),
                block_type: BlockType::Paragraph {
                    paragraph: Default::default(),
                },
                text: "Normal text".to_string(),
                creation_date: Utc::now(),
                update_date: Utc::now(),
                parent_block_id: None,
                has_children: false,
                page_id: "7b1b3b0c-14cb-45a6-a4b6-d2b48faecccb".to_string(),
            },
        ];

        let expected_markdown = "# Heading 1\n## Heading 2\n- Bullet point\nNormal text";
        let result_markdown = blocks
            .iter()
            .map(|block| block.to_markdown())
            .collect::<Vec<String>>()
            .join("\n");

        assert_eq!(result_markdown, expected_markdown);
    }
}
