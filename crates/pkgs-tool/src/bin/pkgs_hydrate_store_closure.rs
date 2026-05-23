#[path = "../common.rs"]
mod common;
#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;
use store_substitute::HydrateClosureArgs;

fn main() {
    let args = HydrateClosureArgs::parse();
    if let Err(error) = store_substitute::hydrate_store_closure(&args) {
        eprintln!("pkgs-hydrate-store-closure: {error}");
        std::process::exit(1);
    }
}
