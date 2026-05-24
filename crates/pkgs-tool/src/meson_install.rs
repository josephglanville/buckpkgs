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

    #[arg(long = "output-path", value_name = "PATH")]
    output_paths: Vec<PathBuf>,

    #[arg(long = "exclude-file-suffix", value_name = "SUFFIX")]
    exclude_file_suffixes: Vec<String>,

    #[arg(long = "split-output", value_name = "NAME=PATH")]
    split_outputs: Vec<String>,

    #[arg(long = "split-output-prefix", value_name = "NAME=PATH")]
    split_output_prefixes: Vec<String>,

    #[arg(long = "split-path", value_name = "NAME=PATH")]
    split_paths: Vec<String>,

    #[arg(long, value_name = "PATH")]
    install_prefix: PathBuf,

    #[arg(long = "path-entry", value_name = "PATH")]
    path_entries: Vec<PathBuf>,

    #[arg(long = "link-input", value_name = "PATH")]
    link_inputs: Vec<PathBuf>,

    #[arg(long = "pkg-config-path", value_name = "PATH")]
    pkg_config_paths: Vec<PathBuf>,

    #[arg(long = "pkg-config-path-for-build", value_name = "PATH")]
    pkg_config_paths_for_build: Vec<PathBuf>,

    #[arg(long = "pkg-config-path-for-target", value_name = "PATH")]
    pkg_config_paths_for_target: Vec<PathBuf>,

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
    let mut env = build::parse_env_assignments(&args.meson_env)?;
    add_link_input_ldflags(&mut env, &args.link_inputs);

    build::apply_patches(&source_dir, &path, &args.patches, args.patch_strip)?;

    let mut setup_command = reproducible_meson_command(args, &path, &env)?;
    setup_command
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
        .args(&args.meson_args);
    common::run_command(&mut setup_command, &args.meson_program)?;

    let mut compile_command = reproducible_meson_command(args, &path, &env)?;
    compile_command
        .arg("compile")
        .arg("-C")
        .arg(&build_dir)
        .arg("--jobs")
        .arg(args.meson_jobs.to_string());
    common::run_command(&mut compile_command, &args.meson_program)?;

    let mut install_command = reproducible_meson_command(args, &path, &env)?;
    install_command
        .arg("install")
        .arg("-C")
        .arg(&build_dir)
        .arg("--destdir")
        .arg(&install_root)
        .arg("--no-rebuild")
        .args(&args.install_args);
    common::run_command(&mut install_command, &args.meson_program)?;

    let split_destinations = build::parse_split_outputs(
        &args.split_outputs,
        &args.split_output_prefixes,
        &args.split_paths,
    )?;
    let (output, split_outputs) = build::staging_outputs(work.path(), &split_destinations);
    build::copy_split_staged_prefix(
        &install_root,
        &args.install_prefix,
        &output,
        &args.output_paths,
        &split_outputs,
    )?;
    for output in std::iter::once(&output).chain(split_outputs.iter().map(|output| &output.output))
    {
        build::exclude_file_suffixes(output, &args.exclude_file_suffixes)?;
        build::sanitize_libtool_archives(output, work.path())?;
        build::sanitize_self_referential_linker_scripts(output, &args.install_prefix)?;
    }
    let output = common::canonicalize(&output)?;
    build::create_symlinks(&output, &args.symlinks)?;
    for output in std::iter::once(&output).chain(split_outputs.iter().map(|output| &output.output))
    {
        common::normalize_tree_mtimes(output)?;
        common::make_tree_read_only(output)?;
    }
    build::publish_sealed_outputs(&output, &args.output, &split_outputs, &split_destinations)?;
    Ok(())
}

fn reproducible_meson_command<'a>(
    args: &'a Args,
    path: &'a std::ffi::OsStr,
    env: &'a [(String, String)],
) -> Result<std::process::Command, common::Error> {
    let mut command = common::reproducible_command(&args.meson_program);
    command
        .env("PATH", path)
        .envs(env.iter().map(|(key, value)| (key, value)));
    common::add_pkg_config_environment(
        &mut command,
        &args.pkg_config_paths,
        &args.pkg_config_paths_for_build,
        &args.pkg_config_paths_for_target,
    )?;
    Ok(command)
}

fn add_link_input_ldflags(env: &mut Vec<(String, String)>, link_inputs: &[PathBuf]) {
    if link_inputs.is_empty() {
        return;
    }

    let flags = link_inputs
        .iter()
        .flat_map(|link_input| {
            [
                format!("-L{}/lib", link_input.display()),
                format!("-Wl,-rpath,{}/lib", link_input.display()),
            ]
        })
        .collect::<Vec<_>>()
        .join(" ");

    if let Some((_, value)) = env.iter_mut().rev().find(|(key, _)| key == "LDFLAGS") {
        if !value.is_empty() {
            value.push(' ');
        }
        value.push_str(&flags);
    } else {
        env.push(("LDFLAGS".to_owned(), flags));
    }
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
