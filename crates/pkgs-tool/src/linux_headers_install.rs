use std::fs;
use std::path::PathBuf;

use clap::Parser;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-linux-headers-install")]
pub(crate) struct Args {
    #[arg(long)]
    source: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long = "path-entry")]
    path_entries: Vec<PathBuf>,

    #[arg(long = "make-arg")]
    make_args: Vec<String>,

    #[arg(long, default_value_t = common::DEFAULT_MAKE_JOBS)]
    make_jobs: usize,

    #[arg(long)]
    kernel_release: String,

    #[arg(long, default_value = "make")]
    make_program: String,
}

pub(crate) fn run(args: &Args) -> Result<(), common::Error> {
    let (work_path, _temp_work) =
        if let Some(path) = common::deterministic_scratch_dir("pkgs-linux-headers-install")? {
            (path, None)
        } else {
            let temp_work = tempfile::Builder::new()
                .prefix("pkgs-linux-headers-install-")
                .tempdir()
                .map_err(|source| common::Error::CreateTempDir { source })?;
            (temp_work.path().to_path_buf(), Some(temp_work))
        };
    let source_dir = work_path.join("source");
    common::copy_tree(&args.source, &source_dir)?;

    fs::create_dir_all(&args.output).map_err(|source| common::Error::CreateDir {
        path: args.output.clone(),
        source,
    })?;
    let output = common::canonicalize(&args.output)?;
    let path = std::env::join_paths(&args.path_entries)
        .map_err(|source| common::Error::JoinPath { source })?;
    let path = common::compiler_wrapped_path(&path, &work_path)?;
    let makeflags = common::makeflags(args.make_jobs)?;

    for target in ["mrproper", "headers"] {
        common::run_command(
            common::reproducible_command(&args.make_program)
                .current_dir(&source_dir)
                .env("PATH", &path)
                .env("MAKEFLAGS", &makeflags)
                .arg(target)
                .args(&args.make_args),
            &args.make_program,
        )?;
    }

    let headers = source_dir.join("usr/include");
    let output_headers = output.join("include");
    common::copy_tree(&headers, &output_headers)?;
    remove_non_headers(&output_headers)?;

    let config_dir = output_headers.join("config");
    fs::create_dir_all(&config_dir).map_err(|source| common::Error::CreateDir {
        path: config_dir.clone(),
        source,
    })?;
    let kernel_release = config_dir.join("kernel.release");
    fs::write(
        &kernel_release,
        format!("{}-default\n", args.kernel_release),
    )
    .map_err(|source| common::Error::WriteFile {
        path: kernel_release,
        source,
    })?;

    common::normalize_tree_mtimes(&output)
}

fn remove_non_headers(path: &std::path::Path) -> Result<(), common::Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            remove_non_headers(&entry.path())?;
        }
        return Ok(());
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("h") {
        fs::remove_file(path).map_err(|source| common::Error::RemoveFile {
            path: path.to_path_buf(),
            source,
        })?;
    }

    Ok(())
}
