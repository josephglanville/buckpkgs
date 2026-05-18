#[path = "../cc_wrapper_tree.rs"]
mod cc_wrapper_tree;
#[path = "../common.rs"]
mod common;

use clap::Parser;

fn main() {
    let args = cc_wrapper_tree::Args::parse();
    if let Err(err) = cc_wrapper_tree::run(&args) {
        eprintln!("pkgs-cc-wrapper-tree: {err}");
        std::process::exit(1);
    }
}
