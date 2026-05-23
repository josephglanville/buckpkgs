#[path = "../common.rs"]
mod common;
#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;
use store_substitute::ExportClosureArgs;

fn main() {
    let args = ExportClosureArgs::parse();
    if let Err(error) = store_substitute::export_store_closure(&args) {
        eprintln!("pkgs-export-store-closure: {error}");
        std::process::exit(1);
    }
}
