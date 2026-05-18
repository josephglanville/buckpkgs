#[path = "../build.rs"]
mod build;
#[path = "../common.rs"]
mod common;
#[path = "../configure_make_install.rs"]
mod configure_make_install;

use clap::Parser;

fn main() {
    let args = configure_make_install::Args::parse();
    if let Err(err) = configure_make_install::run(&args) {
        eprintln!("pkgs-configure-make-install: {err}");
        std::process::exit(1);
    }
}
