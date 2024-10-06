use super::datatypes::Block;
use dendron::{traverse::DftEvent, Tree};

fn build_markdown_from_tree(tree: Tree<Block>, markdown: &mut String) {
    let mut depth = 0;

    for evt in tree.root().depth_first_traverse() {
        match &evt {
            DftEvent::Open(_) => {
                depth += 1;
            }
            DftEvent::Close(_) => {
                depth -= 1;
            }
        }
        let block = evt.as_value().borrow_data();
        println!("{}", serde_json::to_value(block.clone()).unwrap());
        let tabs = "\t".repeat(depth);
        markdown.push_str(&format!("{}{}\n", tabs, block.to_markdown()));
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

#[cfg(test)]
mod tests {
    use dendron::{tree_node, Node};

    use super::*;

    fn fake_tree_for_markdown_building() -> Vec<Tree<Block>> {
        let root1: Tree<Block> = (tree_node! {
          serde_json::from_str(r#"{"block_type":{"paragraph":{"color":"default","rich_text":[{"annotations":{"bold":false,"code":false,"color":"default","italic":false,"strikethrough":false,"underline":false},"plain_text":"11:14: Plan For day:","text":{"content":"11:14: Plan For day:"},"type":"text"}]},"type":"paragraph"},"creation_date":"2024-10-05T15:14:00Z","has_children":true,"id":"1164f233-166c-8100-a937-f753bc111dba","page_id":"1164f233-166c-80f1-88d0-c68546042265","parent_block_id":null,"text":"11:14: Plan For day:","update_date":"2024-10-06T18:51:00Z"}"#).unwrap()
        }).tree();

        vec![root1]
    }

    #[test]
    fn test_build_markdown_from_trees() {}
}
