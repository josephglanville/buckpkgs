#[path = "../common.rs"]
mod common;
#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;
use store_substitute::ProjectHydratedArgs;

fn main() {
    let args = ProjectHydratedArgs::parse();
    if let Err(error) = store_substitute::project_hydrated_store_object(&args) {
        eprintln!("pkgs-project-hydrated-store-object: {error}");
        std::process::exit(1);
    }
}
