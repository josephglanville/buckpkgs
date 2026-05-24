#[path = "../common.rs"]
mod common;
#[path = "../verify_declared_refs.rs"]
mod verify_declared_refs;

use clap::Parser;

fn main() {
    let args = verify_declared_refs::Args::parse();
    if let Err(err) = verify_declared_refs::run(&args) {
        eprintln!("pkgs-verify-declared-refs: {err}");
        std::process::exit(1);
    }
}
