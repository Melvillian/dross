use chrono::Duration;
use dendron::tree::{DebugPrettyPrint, DebugPrintTree};
use dotenv::dotenv;
use dross::{
    core::{datatypes::Block, helpers::build_markdown_from_trees},
    notion::Notion,
};
use log::{debug, info};
use std::{env, fmt::Debug};

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();

    let notion_token: String = env::var("NOTION_TOKEN").expect("NOTION_TOKEN must be set");
    let dur: Duration = Duration::days(match env::var("RUST_LOG") {
        Ok(log_level) => match log_level.to_lowercase().as_str() {
            "debug" => 1,
            _ => 7,
        },
        Err(_) => 7,
    }); // TODO, make this a CLI arg, for now we're just differentiating
        // between DEBUG and non-debug to speed iterating on debugging

    // ingest notes data from Notion
    let notion = Notion::new(notion_token).unwrap();
    let pages_edited_within_dur = notion.get_last_edited_pages(dur).await.unwrap();
    info!(target: "notion", "retrieved {} Pages edited in the last {} days", pages_edited_within_dur.len(), dur.num_days());
    let mut pages_and_block_roots = Vec::new();
    for page in pages_edited_within_dur {
        match notion.get_page_block_roots(&page, dur).await {
            Some(block_roots) => {
                pages_and_block_roots.push((page, block_roots.unwrap()));
            }
            None => {
                continue;
            }
        }
    }

    let mut every_prompt_markdown = Vec::new();
    for (page, block_roots) in pages_and_block_roots {
        let trees = notion.grow_the_roots(block_roots).await.unwrap();
        debug!(target: "notion", "grown {} trees, and they look like:", trees.len());
        debug!(target: "notion", "{:?}", trees);

        let single_page_prompt_markdown = build_markdown_from_trees(trees);
        every_prompt_markdown.push(format!(
            "Page Title: {}\n{:?}",
            page.url, single_page_prompt_markdown
        ));
    }
    let prompt_info = every_prompt_markdown.join("\n\n");
    debug!(target: "notion", "prompt info:\n{}", prompt_info);

    info!(target: "notion", "notion page ingestion successful");
}
