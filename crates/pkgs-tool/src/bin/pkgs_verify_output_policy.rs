#[path = "../common.rs"]
mod common;
#[path = "../verify_output_policy.rs"]
mod verify_output_policy;

use clap::Parser;

fn main() {
    let args = verify_output_policy::Args::parse();
    if let Err(err) = verify_output_policy::run(&args) {
        eprintln!("pkgs-verify-output-policy: {err}");
        std::process::exit(1);
    }
}
