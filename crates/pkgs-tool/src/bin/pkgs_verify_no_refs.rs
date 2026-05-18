#[path = "../common.rs"]
mod common;
#[path = "../verify_no_refs.rs"]
mod verify_no_refs;

use clap::Parser;

fn main() {
    let args = verify_no_refs::Args::parse();
    if let Err(err) = verify_no_refs::run(&args) {
        eprintln!("pkgs-verify-no-refs: {err}");
        std::process::exit(1);
    }
}
