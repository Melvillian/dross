use huramazda_rs::notion::*;
use huramazda_rs::intelligence::*;
use huramazda_rs::core::*;
use dotenv::dotenv;
use std::env;


fn main() {
    dotenv().ok();
    let notion_token: String = env::var("NOTION_TOKEN").expect("NOTION_TOKEN must be set");

    

}
