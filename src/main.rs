use chrono::Duration;
use dotenv::dotenv;
use dross::core::*;
use dross::intelligence::*;
use dross::notion::Notion;
use log::{debug, info};
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    env_logger::init();

    let notion_token: String = env::var("NOTION_TOKEN").expect("NOTION_TOKEN must be set");
    let dur: Duration = Duration::days(7); // TODO, make this a CLI arg

    // ingest notes data from Notion
    let notion = Notion::new(notion_token).unwrap();
    let pages_edited_within_dur = notion.get_last_edited_pages(dur).await.unwrap();
    info!(target: "notion", "retrieved {} Pages edited in the last {} days", pages_edited_within_dur.len(), dur.num_days());
    let retro_blocks = notion
        .pages_to_blocks(pages_edited_within_dur, dur)
        .await
        .unwrap();
    info!(target: "notion", "notion page ingestion successful");
    return;
}
