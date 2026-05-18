#[path = "../bintools_wrapper_tree.rs"]
mod bintools_wrapper_tree;
#[path = "../common.rs"]
mod common;

use clap::Parser;

fn main() {
    let args = bintools_wrapper_tree::Args::parse();
    if let Err(err) = bintools_wrapper_tree::run(&args) {
        eprintln!("pkgs-bintools-wrapper-tree: {err}");
        std::process::exit(1);
    }
}
