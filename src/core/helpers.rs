use super::datatypes::Block;
use dendron::{traverse::DftEvent, Tree};
use log::{debug, trace};

/// Builds Markdown text containing notes that have been edited recently
///
/// It will result in  a markdown file that looks like this:
///
/// ```markdown
/// Page Title: Fix lack of numbering with numbered lists
/// ## Problem
/// Currently the markdown output looks like:
/// But we want it to look like:
///
/// Page Title: Finish All 4 parts of Elliptic Curve Blog Post
/// First Blog Post:  https://andrea.corbellini.name/2015/05/17/elliptic-curve-cryptography-a-gentle-introduction/#comments
///
/// Page Title: Sprint 22
/// ### Planning notes
/// 	- Team availability
/// 		- PTOs
/// 	- Last sprint review
/// 		- What went well
/// 		- What could have gone differently
/// 	- Sprint planning
/// 		- Current sprint goal
/// 		- Commit tasks to sprint
/// ```
fn build_markdown_from_tree(tree: Tree<Block>, markdown: &mut String) {
    let mut depth = 0;

    debug!(
        target: "helpers",
        "building markdown for tree with block id: {:?}",
        tree.root().borrow_data().id
    );

    for evt in tree.root().depth_first_traverse() {
        // see dendron's DFT traversal docs:
        // https://docs.rs/dendron/0.1.5/dendron/node/struct.Node.html#method.depth_first_traverse
        // for how DftEvents work and why we handle DftEvent::Open and DftEvent::Close differently
        match &evt {
            DftEvent::Close(_) => {
                depth -= 1;
            }
            DftEvent::Open(_) => {
                depth += 1;

                let block = evt.as_value().borrow_data();
                trace!(
                    target: "helpers",
                    "DftEvent::Open {:?}",
                    (&block.id, &block.page_id, &block.text)
                );
                let tabs = "\t".repeat(depth);
                markdown.push_str(&format!("{}{}\n", tabs, block.to_markdown()));
            }
        }
    }
    assert!(depth == 0);
}
/// Builds a markdown representation for each Tree in trees by traversing through each
/// tree using DFS (depth first search). The depth of the tree is represented as a number of
/// tabs in front of the line, and each line is a new Block in the Tree
///
/// # Examples
///
/// ```
/// let root1 = tree_node! {
/// Block { text: "Watch General Magic"}, [
///     Block { text: "It's a good documentary"},
///     Block { text: "it's a positive story about technology"},
///     Block { text: "it shows engineer trying to build cool stuff"}, [
///         Block { text: "such as phones"},,
///     ]),
/// ]};
/// let root2 = tree_node! {
/// Block { text: "Cook Dinner"}, [
///     Block { text: "Buy ingredients"},
///     Block { text: "cook them, mash them, stick em in a stew"},
/// ]};
///
/// let markdown = build_markdown_from_trees(vec![root1, root2]);
///
/// assert_eq!(markdown,
/// "Watch General Magic
///     It's a good documentary
///     it's a positive story about technology
///     it shows engineer trying to build cool stuff
///         such as phones
/// Cook Dinner
///     Buy ingredients
///     cook them, mash them, stick em in a stew
/// ");
///
pub fn build_markdown_from_trees(trees: Vec<Tree<Block>>) -> String {
    let mut markdown = String::new();

    for tree in trees {
        build_markdown_from_tree(tree, &mut markdown)
    }

    markdown
}
