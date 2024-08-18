use chrono::Duration;
use dotenv::dotenv;
use huramazda_rs::core::*;
use huramazda_rs::intelligence::*;
use huramazda_rs::notion::Notion;
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let notion_token: String = env::var("NOTION_TOKEN").expect("NOTION_TOKEN must be set");
    let dur = Duration::days(7); // TODO, make this a CLI arg

    let notion = Notion::new(notion_token).unwrap();

    let retro_blocks = notion.get_last_edited_pages(dur).await.unwrap();
    let retro_blocks = notion.pages_to_blocks(retro_blocks, dur).await.unwrap();
    println!("we're finished!");
    return;
}
