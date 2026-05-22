#[path = "../common.rs"]
mod common;

#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;

fn main() {
    let args = store_substitute::HydrateArgs::parse();
    if let Err(err) = store_substitute::hydrate_store_object(&args) {
        eprintln!("pkgs-hydrate-store-object: {err}");
        std::process::exit(1);
    }
}
