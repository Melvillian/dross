use huramazda_rs::notion::{get_last_edited_pages};
use huramazda_rs::intelligence::*;
use huramazda_rs::core::*;
use dotenv::dotenv;
use std::env;
use chrono::Duration;



#[tokio::main]
async fn main() {
    dotenv().ok();
    let notion_token: String = env::var("NOTION_TOKEN").expect("NOTION_TOKEN must be set");
    let dur = Duration::days(7); // TODO, make this a CLI arg

    let retro_blocks = get_last_edited_pages(notion_token, dur).await.unwrap();
    
    // See result
    println!("{:?}", retro_blocks.len());
    print!("{:#?}", retro_blocks);

}
