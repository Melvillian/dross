# Dross (In Rust 🦀)

A tool for expanding the power of your [exobrain](https://beepb00p.xyz/exobrain/) ⚡🧠

# Roadmap

- [ ] Build a service for ingesting exobrain text (Notion, Obsidian, Apple Notes, etc.)
- [ ] Use RAG and LLM Prompting to periodically run a personalized retro for your life
- [ ] Expand memory powers using SRS on exobrain
- [ ] Use Dross to identify [The One Thing](https://en.wikipedia.org/wiki/The_One_Thing_(book)) to iterate on
- [ ] Learn from users what new exobrain powers they would like  

## TODO

- [ ] Ingest latest 7 days of edited blocks from notion
- [ ] Write retro prompt
- [ ] Prompt retro with latest 7 days of exobrain data
- [ ] Build conversation datastructure so I can have a retro with my last 7 days of exobrain data
- [ ] Make more TODOs
- [ ] TODO: need to handle the case where I edit BlockA in PageA, and blockA references as a child BlockB in PageB which I also edited. This is a problem because it means our resulting PromptText will contain duplicate blocks (BlockA and BlockB). It's not the end of the world, but it's not ideal and at least a case for optimization via caching.