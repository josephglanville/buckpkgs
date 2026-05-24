#[path = "../common.rs"]
mod common;
#[path = "../store_substitute.rs"]
mod store_substitute;

use clap::Parser;

fn main() {
    let args = store_substitute::CasManifestArgs::parse();
    if let Err(err) = store_substitute::add_cas_manifest(&args) {
        eprintln!("pkgs-add-cas-manifest: {err}");
        std::process::exit(1);
    }
}
