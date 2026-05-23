use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::{build, common};

#[derive(Debug, Parser)]
#[command(name = "pkgs-meson-install")]
pub(crate) struct Args {
    #[arg(long, value_name = "PATH")]
    source: PathBuf,

    #[arg(long, value_name = "PATH")]
    output: PathBuf,

    #[arg(long, value_name = "PATH")]
    install_prefix: PathBuf,

    #[arg(long = "path-entry", value_name = "PATH")]
    path_entries: Vec<PathBuf>,

    #[arg(long = "link-input", value_name = "PATH")]
    link_inputs: Vec<PathBuf>,

    #[arg(long = "meson-arg", value_name = "ARG")]
    meson_args: Vec<String>,

    #[arg(long = "meson-env", value_name = "KEY=VALUE")]
    meson_env: Vec<String>,

    #[arg(long, default_value_t = common::DEFAULT_MAKE_JOBS, value_name = "N")]
    meson_jobs: usize,

    #[arg(long = "install-arg", value_name = "ARG")]
    install_args: Vec<String>,

    #[arg(long = "patch", value_name = "PATH")]
    patches: Vec<PathBuf>,

    #[arg(long, default_value_t = 1, value_name = "N")]
    patch_strip: u8,

    #[arg(long = "symlink", value_name = "LINK=TARGET")]
    symlinks: Vec<String>,

    #[arg(long, default_value = "meson", value_name = "PROGRAM")]
    meson_program: String,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Build(#[from] build::Error),

    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("meson job count must be at least one")]
    InvalidMesonJobs,
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    if args.meson_jobs == 0 {
        return Err(Error::InvalidMesonJobs);
    }

    let work = WorkDir::new()?;
    let source_dir = work.path().join("source");
    let build_dir = work.path().join("build");
    let install_root = work.path().join("install-root");
    common::copy_tree(&args.source, &source_dir)?;
    fs::create_dir(&build_dir).map_err(|source| common::Error::CreateDir {
        path: build_dir.clone(),
        source,
    })?;
    fs::create_dir(&install_root).map_err(|source| common::Error::CreateDir {
        path: install_root.clone(),
        source,
    })?;

    let path = std::env::join_paths(&args.path_entries)
        .map_err(|source| common::Error::JoinPath { source })?;
    let path = common::compiler_wrapped_path(&path, work.path(), &args.link_inputs)?;
    let env = build::parse_env_assignments(&args.meson_env)?;

    build::apply_patches(&source_dir, &path, &args.patches, args.patch_strip)?;

    common::run_command(
        reproducible_meson_command(args, &path, &env)
            .arg("setup")
            .arg(&build_dir)
            .arg(&source_dir)
            .arg(format!("--prefix={}", args.install_prefix.display()))
            .arg("--libdir=lib")
            .arg("--backend=ninja")
            .arg("--buildtype=plain")
            .arg("--auto-features=disabled")
            .arg("--wrap-mode=nodownload")
            .arg("--install-umask=022")
            .args(&args.meson_args),
        &args.meson_program,
    )?;

    common::run_command(
        reproducible_meson_command(args, &path, &env)
            .arg("compile")
            .arg("-C")
            .arg(&build_dir)
            .arg("--jobs")
            .arg(args.meson_jobs.to_string()),
        &args.meson_program,
    )?;

    common::run_command(
        reproducible_meson_command(args, &path, &env)
            .arg("install")
            .arg("-C")
            .arg(&build_dir)
            .arg("--destdir")
            .arg(&install_root)
            .arg("--no-rebuild")
            .args(&args.install_args),
        &args.meson_program,
    )?;

    build::copy_staged_prefix(&install_root, &args.install_prefix, &args.output)?;
    build::sanitize_libtool_archives(&args.output, work.path())?;
    build::sanitize_self_referential_linker_scripts(&args.output, &args.install_prefix)?;
    let output = common::canonicalize(&args.output)?;
    build::create_symlinks(&output, &args.symlinks)?;
    common::normalize_tree_mtimes(&output)?;
    common::make_tree_read_only(&output)?;
    Ok(())
}

fn reproducible_meson_command<'a>(
    args: &'a Args,
    path: &'a std::ffi::OsStr,
    env: &'a [(String, String)],
) -> std::process::Command {
    let mut command = common::reproducible_command(&args.meson_program);
    command
        .env("PATH", path)
        .envs(env.iter().map(|(key, value)| (key, value)));
    command
}

enum WorkDir {
    Temp(tempfile::TempDir),
    Persistent(PathBuf),
}

impl WorkDir {
    fn new() -> Result<Self, Error> {
        if let Some(path) = common::deterministic_scratch_dir("pkgs-meson-install")? {
            return Ok(Self::Persistent(path));
        }

        tempfile::Builder::new()
            .prefix("pkgs-meson-install-")
            .tempdir()
            .map(Self::Temp)
            .map_err(|source| common::Error::CreateTempDir { source }.into())
    }

    fn path(&self) -> &Path {
        match self {
            Self::Temp(path) => path.path(),
            Self::Persistent(path) => path,
        }
    }
}
