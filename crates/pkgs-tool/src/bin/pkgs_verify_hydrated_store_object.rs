#[path = "../common.rs"]
mod common;
#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;
use store_substitute::VerifyHydratedArgs;

fn main() {
    let args = VerifyHydratedArgs::parse();
    if let Err(error) = store_substitute::verify_hydrated_store_object(&args) {
        eprintln!("pkgs-verify-hydrated-store-object: {error}");
        std::process::exit(1);
    }
}
