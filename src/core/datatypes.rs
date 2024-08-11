use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]

pub struct Block {
    pub id: String,
    pub raw_text: String,
    pub creation_date: DateTime<Utc>,
    pub creator: String,
    pub update_date: DateTime<Utc>,
    pub last_editor: String,
    pub tags: Vec<String>,
    pub parent_block_id: Option<String>,
    pub child_block_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BlockText {
    pub block: Block,
    pub formatting: Option<TextFormatting>,
}

#[derive(Debug, Clone)]
pub struct TextFormatting {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub font_size: u8,
    pub font_family: String,
}

#[derive(Debug, Clone)]
pub struct BlockImage {









    pub block: Block,
    pub image_url: String,
}