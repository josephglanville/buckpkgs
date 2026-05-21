use clap::Parser;

#[path = "../common.rs"]
mod common;
#[path = "../verify_archive_metadata.rs"]
mod verify_archive_metadata;

fn main() {
    let args = verify_archive_metadata::Args::parse();
    if let Err(err) = verify_archive_metadata::run(&args) {
        eprintln!("pkgs-verify-archive-metadata: {err}");
        std::process::exit(1);
    }
}
