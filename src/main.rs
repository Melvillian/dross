use chrono::Duration;
use dotenv::dotenv;
use dross::{core::helpers::build_markdown_from_trees, notion::Notion};
use log::{debug, info};
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();

    let dur: Duration = Duration::days(match env::var("RUST_LOG") {
        Ok(log_level) => match log_level.to_lowercase().as_str() {
            "debug" | "trace" => 1,
            _ => 7,
        },
        Err(_) => 7,
    }); // TODO, make this a CLI arg, for now we're just differentiating
        // between DEBUG and non-debug to speed iterating on debugging

    // ingest notes data from Notion
    let notion_token: String = env::var("NOTION_TOKEN").expect("NOTION_TOKEN must be set");
    let notion = Notion::new(notion_token).unwrap();

    let pages_edited_within_dur = notion.get_last_edited_pages(dur).await.unwrap();
    info!(target: "notion", "retrieved {} Pages edited in the last {} days", pages_edited_within_dur.len(), dur.num_days());
    let mut pages_and_block_roots = Vec::new();
    for page in pages_edited_within_dur {
        debug!(target: "notion", "Page URL: {}", page.url);

        let new_block_roots = notion.get_page_block_roots(&page, dur).await.unwrap();
        pages_and_block_roots.push((page, new_block_roots));
    }

    debug!(target: "notion", "retrieved {} pages and their block roots, now we will grow them!", pages_and_block_roots.len());

    let mut every_prompt_markdown = Vec::new();
    for (page, block_roots) in pages_and_block_roots {
        let trees = notion.grow_the_roots(block_roots).await.unwrap();
        debug!(target: "notion", "grown {} trees, and they look like:", trees.len());
        debug!(target: "notion", "{:?}", trees);

        let single_page_prompt_markdown = build_markdown_from_trees(trees);
        every_prompt_markdown.push(format!(
            "Page Title: {}\n{:?}",
            page.title, single_page_prompt_markdown
        ));
    }
    let prompt_info = every_prompt_markdown.join("\n\n");
    debug!(target: "notion", "prompt info:\n{}", prompt_info);

    info!(target: "notion", "notion page ingestion successful");
}
