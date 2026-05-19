#[path = "../stage_tree.rs"]
mod stage_tree;

#[path = "../common.rs"]
mod common;

use clap::Parser;

fn main() {
    let args = stage_tree::Args::parse();
    if let Err(err) = stage_tree::run(&args) {
        eprintln!("pkgs-stage-tree: {err}");
        std::process::exit(1);
    }
}
