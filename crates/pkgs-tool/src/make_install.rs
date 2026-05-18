use std::fs;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use clap::Parser;
use thiserror::Error;

use crate::{build, common};

#[derive(Debug, Parser)]
#[command(name = "pkgs-make-install")]
pub(crate) struct Args {
    #[arg(long)]
    source: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long)]
    install_prefix: PathBuf,

    #[arg(long = "path-entry")]
    path_entries: Vec<PathBuf>,

    #[arg(long = "make-arg")]
    make_args: Vec<String>,

    #[arg(long = "install-arg")]
    install_args: Vec<String>,

    #[arg(long = "patch")]
    patches: Vec<PathBuf>,

    #[arg(long, default_value_t = 1)]
    patch_strip: u8,

    #[arg(long = "symlink")]
    symlinks: Vec<String>,

    #[arg(long, default_value = "make")]
    make_program: String,

    #[arg(long, default_value = "PREFIX")]
    prefix_var: String,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Build(#[from] build::Error),

    #[error(transparent)]
    Common(#[from] common::Error),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    let work = tempfile::Builder::new()
        .prefix("pkgs-make-install-")
        .tempdir()
        .map_err(|source| common::Error::CreateTempDir { source })?;
    let source_dir = work.path().join("source");
    common::copy_tree(&args.source, &source_dir)?;

    let install_root = work.path().join("install-root");
    fs::create_dir(&install_root).map_err(|source| common::Error::CreateDir {
        path: install_root.clone(),
        source,
    })?;

    let path = std::env::join_paths(&args.path_entries)
        .map_err(|source| common::Error::JoinPath { source })?;
    let makeflags = format!("-j{}", common::available_jobs());

    build::apply_patches(&source_dir, &path, &args.patches, args.patch_strip)?;

    common::run_command(
        ProcessCommand::new(&args.make_program)
            .current_dir(&source_dir)
            .env("PATH", &path)
            .env("MAKEFLAGS", &makeflags)
            .args(&args.make_args),
        &args.make_program,
    )?;

    let prefix_arg = format!("{}={}", args.prefix_var, args.install_prefix.display());
    common::run_command(
        ProcessCommand::new(&args.make_program)
            .current_dir(&source_dir)
            .env("PATH", &path)
            .env("MAKEFLAGS", &makeflags)
            .arg("install")
            .arg(prefix_arg)
            .arg(format!("DESTDIR={}", install_root.display()))
            .args(&args.install_args),
        &args.make_program,
    )?;

    build::copy_staged_prefix(&install_root, &args.install_prefix, &args.output)?;
    let output = common::canonicalize(&args.output)?;
    build::create_symlinks(&output, &args.symlinks)?;
    Ok(())
}
