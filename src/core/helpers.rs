use dendron::{Node, Tree};

use super::datatypes::Block;

pub fn build_markdown_from_trees(trees: Vec<Tree<Block>>) -> String {
    let mut markdown = String::new();

    for tree in trees {
        build_markdown_recursive(tree.root(), 0, &mut markdown);
    }

    markdown
}

fn build_markdown_recursive(node: Node<Block>, depth: usize, markdown: &mut String) {
    let tabs = "\t".repeat(depth);
    markdown.push_str(&format!("{}{}\n", tabs, node.borrow_data().text));

    // println!("{}", &format!("{}{}\n", tabs, node.borrow_data().text));
    // println!("{}", node.)

    for child in node.children() {
        build_markdown_recursive(child, depth + 1, markdown);
    }
}
