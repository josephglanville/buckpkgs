use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::{build, common};

const WORK_DIR_PLACEHOLDER: &str = "@PKGS_WORK_DIR@";

#[derive(Debug, Parser)]
#[command(name = "pkgs-configure-make-install")]
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

    #[arg(long = "configure-arg")]
    configure_args: Vec<String>,

    #[arg(long = "configure-env")]
    configure_env: Vec<String>,

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

    #[arg(long, default_value = "./configure")]
    configure_program: String,

    #[arg(long, default_value = "make")]
    make_program: String,

    #[arg(long)]
    out_of_source: bool,

    #[arg(long)]
    work_dir: Option<PathBuf>,

    #[arg(long)]
    reuse_work_dir: bool,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Build(#[from] build::Error),

    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("work directory already exists; pass --reuse-work-dir to reuse it: {0}")]
    WorkDirExists(PathBuf),

    #[error("cannot pass --reuse-work-dir without --work-dir")]
    ReuseWorkDirWithoutWorkDir,

    #[error("reused work directory is missing expected path {0}")]
    MissingWorkDirPath(PathBuf),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    let work = WorkDir::new(args)?;
    let source_dir = work.path().join("source");
    if !work.is_reused() {
        common::copy_tree(&args.source, &source_dir)?;
    }
    let build_dir = if args.out_of_source {
        let build_dir = work.path().join("build");
        if work.is_reused() {
            require_existing_dir(&build_dir)?;
        } else {
            fs::create_dir(&build_dir).map_err(|source| common::Error::CreateDir {
                path: build_dir.clone(),
                source,
            })?;
        }
        build_dir
    } else {
        if work.is_reused() {
            require_existing_dir(&source_dir)?;
        }
        source_dir.clone()
    };

    let install_root = work.path().join("install-root");
    if install_root.exists() {
        fs::remove_dir_all(&install_root).map_err(|source| common::Error::RemoveDir {
            path: install_root.clone(),
            source,
        })?;
    }
    fs::create_dir(&install_root).map_err(|source| common::Error::CreateDir {
        path: install_root.clone(),
        source,
    })?;

    let path = std::env::join_paths(&args.path_entries)
        .map_err(|source| common::Error::JoinPath { source })?;
    let path = common::compiler_wrapped_path(&path, work.path(), &args.link_inputs)?;
    let configure_args = expand_work_dir_placeholders(&args.configure_args, work.path());
    let configure_env = expand_work_dir_placeholders(&args.configure_env, work.path());
    let env = build::parse_env_assignments(&configure_env)?;
    let makeflags = common::makeflags(args.make_jobs)?;
    let prefix_arg = format!("--prefix={}", args.install_prefix.display());
    let configure_program =
        configure_program(&args.configure_program, &source_dir, args.out_of_source);

    if !work.is_reused() {
        build::apply_patches(&source_dir, &path, &args.patches, args.patch_strip)?;
    }

    common::run_command(
        common::reproducible_command(&configure_program)
            .current_dir(&build_dir)
            .env("PATH", &path)
            .envs(env.iter().map(|(key, value)| (key, value)))
            .arg(prefix_arg)
            .args(&configure_args),
        &configure_program.display().to_string(),
    )?;

    common::run_command(
        common::reproducible_command(&args.make_program)
            .current_dir(&build_dir)
            .env("PATH", &path)
            .env("MAKEFLAGS", &makeflags)
            .envs(env.iter().map(|(key, value)| (key, value)))
            .args(&args.make_args),
        &args.make_program,
    )?;

    common::run_command(
        common::reproducible_command(&args.make_program)
            .current_dir(&build_dir)
            .env("PATH", &path)
            .env("MAKEFLAGS", &makeflags)
            .envs(env.iter().map(|(key, value)| (key, value)))
            .arg("install")
            .arg(format!("DESTDIR={}", install_root.display()))
            .args(&args.install_args),
        &args.make_program,
    )?;

    build::copy_staged_prefix(&install_root, &args.install_prefix, &args.output)?;
    build::sanitize_libtool_archives(&args.output, work.path())?;
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

fn configure_program(program: &str, source_dir: &Path, out_of_source: bool) -> PathBuf {
    let program = Path::new(program);
    if out_of_source && !program.is_absolute() {
        if let Some(source_dir_name) = source_dir.file_name() {
            return PathBuf::from("..").join(source_dir_name).join(program);
        }

        return source_dir.join(program);
    }

    program.to_path_buf()
}

fn expand_work_dir_placeholders(values: &[String], work_dir: &Path) -> Vec<String> {
    let work_dir = work_dir.display().to_string();
    values
        .iter()
        .map(|value| value.replace(WORK_DIR_PLACEHOLDER, &work_dir))
        .collect()
}

enum WorkDir {
    Temp(tempfile::TempDir),
    Persistent { path: PathBuf, reused: bool },
}

impl WorkDir {
    fn new(args: &Args) -> Result<Self, Error> {
        match (&args.work_dir, args.reuse_work_dir) {
            (None, false) => {
                if let Some(path) =
                    common::deterministic_scratch_dir("pkgs-configure-make-install")?
                {
                    return Ok(WorkDir::Persistent {
                        path,
                        reused: false,
                    });
                }

                tempfile::Builder::new()
                    .prefix("pkgs-configure-make-install-")
                    .tempdir()
                    .map(WorkDir::Temp)
                    .map_err(|source| common::Error::CreateTempDir { source }.into())
            }
            (None, true) => Err(Error::ReuseWorkDirWithoutWorkDir),
            (Some(path), reuse) => {
                if path.exists() {
                    if !reuse {
                        return Err(Error::WorkDirExists(path.clone()));
                    }
                    require_existing_dir(path)?;
                    return Ok(WorkDir::Persistent {
                        path: path.clone(),
                        reused: true,
                    });
                }

                fs::create_dir_all(path).map_err(|source| common::Error::CreateDir {
                    path: path.clone(),
                    source,
                })?;
                Ok(WorkDir::Persistent {
                    path: path.clone(),
                    reused: false,
                })
            }
        }
    }

    fn path(&self) -> &Path {
        match self {
            WorkDir::Temp(path) => path.path(),
            WorkDir::Persistent { path, .. } => path,
        }
    }

    fn is_reused(&self) -> bool {
        matches!(self, WorkDir::Persistent { reused: true, .. })
    }
}

fn require_existing_dir(path: &Path) -> Result<(), Error> {
    if path.is_dir() {
        return Ok(());
    }

    Err(Error::MissingWorkDirPath(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_source_configure_program_stays_relative() {
        assert_eq!(
            configure_program("./configure", Path::new("/tmp/work/source"), true),
            PathBuf::from("../source/./configure"),
        );
    }

    #[test]
    fn work_dir_placeholders_expand_in_order() {
        assert_eq!(
            expand_work_dir_placeholders(
                &[
                    "--with-debug-prefix-map=@PKGS_WORK_DIR@=.".to_owned(),
                    "CC=@PKGS_WORK_DIR@/bin/cc".to_owned(),
                ],
                Path::new("/tmp/work"),
            ),
            vec![
                "--with-debug-prefix-map=/tmp/work=.".to_owned(),
                "CC=/tmp/work/bin/cc".to_owned(),
            ],
        );
    }
}
