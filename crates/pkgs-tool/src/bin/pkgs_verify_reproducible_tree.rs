#[path = "../common.rs"]
mod common;
#[path = "../verify_reproducible_tree.rs"]
mod verify_reproducible_tree;

use clap::Parser;

fn main() {
    let args = verify_reproducible_tree::Args::parse();
    if let Err(err) = verify_reproducible_tree::run(&args) {
        eprintln!("pkgs-verify-reproducible-tree: {err}");
        std::process::exit(1);
    }
}
