#[path = "../compose_sources.rs"]
mod compose_sources;

#[path = "../common.rs"]
mod common;

use clap::Parser;

fn main() {
    let args = compose_sources::Args::parse();
    if let Err(err) = compose_sources::run(&args) {
        eprintln!("pkgs-compose-sources: {err}");
        std::process::exit(1);
    }
}
