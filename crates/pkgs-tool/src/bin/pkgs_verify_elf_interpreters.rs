#[path = "../common.rs"]
mod common;
#[path = "../verify_elf_interpreters.rs"]
mod verify_elf_interpreters;

use clap::Parser;

fn main() {
    let args = verify_elf_interpreters::Args::parse();
    if let Err(err) = verify_elf_interpreters::run(&args) {
        eprintln!("pkgs-verify-elf-interpreters: {err}");
        std::process::exit(1);
    }
}
