use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-output-policy")]
pub(crate) struct Args {
    #[arg(long)]
    input: Vec<PathBuf>,

    #[arg(long = "allow-runtime-path")]
    allowed_runtime_paths: Vec<PathBuf>,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("runtime-data exception path must be relative and stay within the output tree: {0}")]
    InvalidAllowedRuntimePath(PathBuf),

    #[error("published output retains non-code payload {path}; declare a runtime-data exception if required")]
    ForbiddenPayload { path: PathBuf },
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    for path in &args.allowed_runtime_paths {
        validate_relative_path(path)?;
    }
    for input in &args.input {
        verify_tree(input, input, &args.allowed_runtime_paths)?;
    }
    write_stamp(&args.stamp)?;
    Ok(())
}

fn verify_tree(root: &Path, path: &Path, allowed_runtime_paths: &[PathBuf]) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in common::sorted_dir_entries(path)? {
            verify_tree(root, &entry.path(), allowed_runtime_paths)?;
        }
        return Ok(());
    }

    let relative = path.strip_prefix(root).unwrap_or(path);
    if is_data_payload(relative)
        && !allowed_runtime_paths
            .iter()
            .any(|allowed| relative.starts_with(allowed))
    {
        return Err(Error::ForbiddenPayload {
            path: relative.to_path_buf(),
        });
    }
    Ok(())
}

fn is_data_payload(path: &Path) -> bool {
    ["share", "man", "doc", "info"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn validate_relative_path(path: &Path) -> Result<(), Error> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidAllowedRuntimePath(path.to_path_buf()));
    }
    Ok(())
}

fn write_stamp(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, "ok\n").map_err(|source| common::Error::WriteStamp {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn rejects_documentation_payloads_by_default() {
        let temp = tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir_all(tree.join("share/doc/pkg")).unwrap();
        fs::write(tree.join("share/doc/pkg/README"), "documentation").unwrap();

        let result = run(&Args {
            input: vec![tree],
            allowed_runtime_paths: vec![],
            stamp: temp.path().join("stamp"),
        });
        assert!(matches!(result, Err(Error::ForbiddenPayload { .. })));
    }

    #[test]
    fn allows_explicit_runtime_data_paths() {
        let temp = tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir_all(tree.join("share/locale/C")).unwrap();
        fs::write(tree.join("share/locale/C/messages"), "runtime data").unwrap();

        run(&Args {
            input: vec![tree],
            allowed_runtime_paths: vec![PathBuf::from("share/locale")],
            stamp: temp.path().join("stamp"),
        })
        .unwrap();
    }

    #[test]
    fn accepts_code_payloads_without_exceptions() {
        let temp = tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir_all(tree.join("lib")).unwrap();
        fs::write(tree.join("lib/libexample.so"), "runtime").unwrap();

        run(&Args {
            input: vec![tree],
            allowed_runtime_paths: vec![],
            stamp: temp.path().join("stamp"),
        })
        .unwrap();
    }
}
