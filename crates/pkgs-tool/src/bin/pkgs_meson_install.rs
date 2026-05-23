#[path = "../build.rs"]
mod build;
#[path = "../common.rs"]
mod common;
#[path = "../meson_install.rs"]
mod meson_install;

use clap::Parser;

fn main() {
    let args = meson_install::Args::parse();
    if let Err(err) = meson_install::run(&args) {
        eprintln!("pkgs-meson-install: {err}");
        std::process::exit(1);
    }
}
