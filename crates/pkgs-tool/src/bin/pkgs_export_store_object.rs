#[path = "../common.rs"]
mod common;

#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;

fn main() {
    let args = store_substitute::ExportArgs::parse();
    if let Err(err) = store_substitute::export_store_object(&args) {
        eprintln!("pkgs-export-store-object: {err}");
        std::process::exit(1);
    }
}
