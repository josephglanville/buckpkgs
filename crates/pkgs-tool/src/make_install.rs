use std::fs;
use std::path::PathBuf;

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

    #[arg(long = "link-input")]
    link_inputs: Vec<PathBuf>,

    #[arg(long = "make-arg")]
    make_args: Vec<String>,

    #[arg(long, default_value_t = common::DEFAULT_MAKE_JOBS)]
    make_jobs: usize,

    #[arg(long = "install-arg")]
    install_args: Vec<String>,

    #[arg(long = "python-bytecode-dir")]
    python_bytecode_dirs: Vec<PathBuf>,

    #[arg(long = "python-bytecode-interpreter")]
    python_bytecode_interpreter: Option<PathBuf>,

    #[arg(long = "python-bytecode-self-interpreter")]
    python_bytecode_self_interpreter: Option<PathBuf>,

    #[arg(long = "python-bytecode-optimize")]
    python_bytecode_optimizations: Vec<u8>,

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
    let (work_path, _temp_work) =
        if let Some(path) = common::deterministic_scratch_dir("pkgs-make-install")? {
            (path, None)
        } else {
            let temp_work = tempfile::Builder::new()
                .prefix("pkgs-make-install-")
                .tempdir()
                .map_err(|source| common::Error::CreateTempDir { source })?;
            (temp_work.path().to_path_buf(), Some(temp_work))
        };
    let source_dir = work_path.join("source");
    common::copy_tree(&args.source, &source_dir)?;

    let install_root = work_path.join("install-root");
    fs::create_dir(&install_root).map_err(|source| common::Error::CreateDir {
        path: install_root.clone(),
        source,
    })?;

    let path = std::env::join_paths(&args.path_entries)
        .map_err(|source| common::Error::JoinPath { source })?;
    let path = common::compiler_wrapped_path(&path, &work_path, &args.link_inputs)?;
    let makeflags = common::makeflags(args.make_jobs)?;

    build::apply_patches(&source_dir, &path, &args.patches, args.patch_strip)?;

    common::run_command(
        common::reproducible_command(&args.make_program)
            .current_dir(&source_dir)
            .env("PATH", &path)
            .env("MAKEFLAGS", &makeflags)
            .args(&args.make_args),
        &args.make_program,
    )?;

    let prefix_arg = format!("{}={}", args.prefix_var, args.install_prefix.display());
    common::run_command(
        common::reproducible_command(&args.make_program)
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
    build::sanitize_libtool_archives(&args.output, &work_path)?;
    build::sanitize_self_referential_linker_scripts(&args.output, &args.install_prefix)?;
    let output = common::canonicalize(&args.output)?;
    build::create_symlinks(&output, &args.symlinks)?;
    build::compile_python_bytecode(
        &output,
        &args.install_prefix,
        args.python_bytecode_interpreter.as_deref(),
        args.python_bytecode_self_interpreter.as_deref(),
        &args.python_bytecode_dirs,
        &args.python_bytecode_optimizations,
    )?;
    common::normalize_tree_mtimes(&output)?;
    common::make_tree_read_only(&output)?;
    Ok(())
}
