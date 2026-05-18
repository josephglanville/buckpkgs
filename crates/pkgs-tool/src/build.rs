#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use thiserror::Error;

use crate::common;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("invalid environment assignment, expected KEY=VALUE: {0}")]
    InvalidEnvAssignment(String),

    #[error("invalid symlink assignment, expected LINK=TARGET: {0}")]
    InvalidSymlinkAssignment(String),

    #[error("path must be relative and stay within the output tree: {0}")]
    InvalidRelativePath(PathBuf),

    #[error("install prefix must be absolute: {0}")]
    InvalidInstallPrefix(PathBuf),
}

pub(crate) fn parse_env_assignments(
    assignments: &[String],
) -> Result<Vec<(String, String)>, Error> {
    assignments
        .iter()
        .map(|assignment| {
            assignment
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .ok_or_else(|| Error::InvalidEnvAssignment(assignment.clone()))
        })
        .collect()
}

pub(crate) fn create_symlinks(output: &Path, assignments: &[String]) -> Result<(), Error> {
    for assignment in assignments {
        let (link, target) = assignment
            .split_once('=')
            .ok_or_else(|| Error::InvalidSymlinkAssignment(assignment.clone()))?;
        let link = PathBuf::from(link);
        let target = PathBuf::from(target);
        validate_relative_path(&link)?;
        validate_relative_path(&target)?;

        let link_path = output.join(&link);
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        symlink(&target, &link_path).map_err(|source| common::Error::CreateSymlink {
            from: target,
            to: link_path,
            source,
        })?;
    }

    Ok(())
}

pub(crate) fn apply_patches(
    source_dir: &Path,
    path: &std::ffi::OsStr,
    patches: &[PathBuf],
    patch_strip: u8,
) -> Result<(), Error> {
    for patch in patches {
        let patch = common::canonicalize(patch)?;
        common::run_command(
            ProcessCommand::new("patch")
                .current_dir(source_dir)
                .env("PATH", path)
                .arg(format!("-p{patch_strip}"))
                .arg("-i")
                .arg(patch),
            "patch",
        )?;
    }

    Ok(())
}

pub(crate) fn copy_staged_prefix(
    install_root: &Path,
    install_prefix: &Path,
    output: &Path,
) -> Result<(), Error> {
    let relative_prefix = install_prefix
        .strip_prefix("/")
        .map_err(|_| Error::InvalidInstallPrefix(install_prefix.to_path_buf()))?;
    let staged_prefix = install_root.join(relative_prefix);
    common::copy_tree(&staged_prefix, output)?;
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<(), Error> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidRelativePath(path.to_path_buf()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_environment_assignments() {
        assert_eq!(
            parse_env_assignments(&["CC=cc".to_owned(), "AR=ar".to_owned()]).unwrap(),
            vec![
                ("CC".to_owned(), "cc".to_owned()),
                ("AR".to_owned(), "ar".to_owned()),
            ]
        );
        assert!(parse_env_assignments(&["BROKEN".to_owned()]).is_err());
    }

    #[test]
    fn rejects_non_relative_output_paths() {
        assert!(validate_relative_path(Path::new("bin/sh")).is_ok());
        assert!(validate_relative_path(Path::new("/bin/sh")).is_err());
        assert!(validate_relative_path(Path::new("../bin/sh")).is_err());
    }
}
