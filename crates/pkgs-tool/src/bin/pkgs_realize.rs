#[path = "../common.rs"]
mod common;
#[path = "../realize.rs"]
mod realize;

use clap::Parser;

fn main() {
    let args = realize::Args::parse();
    if let Err(err) = realize::run(&args) {
        eprintln!("pkgs-realize: {err}");
        std::process::exit(1);
    }
}
