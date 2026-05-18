use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-realize")]
pub(crate) struct Args {
    #[arg(long)]
    source: PathBuf,

    #[arg(long)]
    store_root: PathBuf,

    #[arg(long)]
    store_path: String,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("store path must be a single relative component: {0}")]
    InvalidStorePath(String),

    #[error("source tree does not exist: {0}")]
    MissingSource(PathBuf),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    validate_store_path(&args.store_path)?;

    if !args.source.exists() {
        return Err(Error::MissingSource(args.source.clone()));
    }

    fs::create_dir_all(&args.store_root).map_err(|source| common::Error::CreateDir {
        path: args.store_root.clone(),
        source,
    })?;

    let destination = args.store_root.join(&args.store_path);
    if !destination.exists() {
        common::copy_tree(&args.source, &destination)?;
        make_tree_read_only(&destination)?;
    }

    if let Some(parent) = args.stamp.parent() {
        fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    fs::write(&args.stamp, destination.display().to_string()).map_err(|source| {
        common::Error::WriteStamp {
            path: args.stamp.clone(),
            source,
        }
    })?;

    Ok(())
}

fn validate_store_path(store_path: &str) -> Result<(), Error> {
    let path = Path::new(store_path);
    let mut components = path.components();

    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(()),
        _ => Err(Error::InvalidStorePath(store_path.to_owned())),
    }
}

fn make_tree_read_only(path: &Path) -> Result<(), common::Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        let entries = fs::read_dir(path).map_err(|source| common::Error::ReadDir {
            path: path.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| common::Error::ReadDir {
                path: path.to_path_buf(),
                source,
            })?;
            make_tree_read_only(&entry.path())?;
        }
    }

    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(path, permissions).map_err(|source| common::Error::SetPermissions {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nested_store_paths() {
        assert!(validate_store_path("abc-bash").is_ok());
        assert!(validate_store_path("../abc-bash").is_err());
        assert!(validate_store_path("nested/abc-bash").is_err());
        assert!(validate_store_path("/abc-bash").is_err());
    }
}
