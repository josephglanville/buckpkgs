use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-compose-sources")]
pub(crate) struct Args {
    #[arg(long = "source", required = true)]
    sources: Vec<PathBuf>,

    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("source tree does not exist: {0}")]
    MissingSource(PathBuf),

    #[error("source input is not a directory: {0}")]
    SourceIsNotDirectory(PathBuf),

    #[error("composed source output already exists: {0}")]
    ExistingOutput(PathBuf),

    #[error("failed to inspect composed source path {path}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("source composition conflict at {0}")]
    Conflict(PathBuf),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    if args.output.exists() {
        return Err(Error::ExistingOutput(args.output.clone()));
    }

    let (first, rest) = args
        .sources
        .split_first()
        .expect("clap enforces at least one source");
    validate_source(first)?;
    common::copy_tree(first, &args.output)?;

    for source in rest {
        validate_source(source)?;
        merge_tree(source, &args.output)?;
    }

    Ok(())
}

fn validate_source(source: &Path) -> Result<(), Error> {
    if !source.exists() {
        return Err(Error::MissingSource(source.to_path_buf()));
    }

    let metadata = fs::symlink_metadata(source).map_err(|source_err| Error::Metadata {
        path: source.to_path_buf(),
        source: source_err,
    })?;
    if !metadata.is_dir() {
        return Err(Error::SourceIsNotDirectory(source.to_path_buf()));
    }

    Ok(())
}

fn merge_tree(source: &Path, destination: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(destination) {
        Err(source_err) if source_err.kind() == io::ErrorKind::NotFound => {
            common::copy_tree(source, destination)?;
            return Ok(());
        }
        Err(source_err) => {
            return Err(Error::Metadata {
                path: destination.to_path_buf(),
                source: source_err,
            });
        }
        Ok(destination_metadata) => {
            let source_metadata =
                fs::symlink_metadata(source).map_err(|source_err| Error::Metadata {
                    path: source.to_path_buf(),
                    source: source_err,
                })?;
            if !source_metadata.is_dir() || !destination_metadata.is_dir() {
                return Err(Error::Conflict(destination.to_path_buf()));
            }
        }
    }

    for entry in common::sorted_dir_entries(source)? {
        merge_tree(&entry.path(), &destination.join(entry.file_name()))?;
    }

    Ok(())
}
