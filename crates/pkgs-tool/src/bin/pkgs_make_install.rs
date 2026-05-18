#[path = "../build.rs"]
mod build;
#[path = "../common.rs"]
mod common;
#[path = "../make_install.rs"]
mod make_install;

use clap::Parser;

fn main() {
    let args = make_install::Args::parse();
    if let Err(err) = make_install::run(&args) {
        eprintln!("pkgs-make-install: {err}");
        std::process::exit(1);
    }
}
