#[path = "../common.rs"]
mod common;
#[path = "../linux_headers_install.rs"]
mod linux_headers_install;

use clap::Parser;

fn main() {
    let args = linux_headers_install::Args::parse();
    if let Err(err) = linux_headers_install::run(&args) {
        eprintln!("pkgs-linux-headers-install: {err}");
        std::process::exit(1);
    }
}
