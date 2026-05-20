#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

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
            common::reproducible_command("patch")
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

pub(crate) fn sanitize_libtool_archives(output: &Path, work_dir: &Path) -> Result<(), Error> {
    sanitize_libtool_archives_in_tree(output, work_dir)
}

fn sanitize_libtool_archives_in_tree(path: &Path, work_dir: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            sanitize_libtool_archives_in_tree(&entry.path(), work_dir)?;
        }
        return Ok(());
    }

    if path.extension().and_then(|extension| extension.to_str()) != Some("la") {
        return Ok(());
    }

    let contents = fs::read_to_string(path).map_err(|source| common::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(rewritten) = rewrite_libtool_archive(&contents, work_dir) else {
        return Ok(());
    };

    fs::write(path, rewritten).map_err(|source| common::Error::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;
    common::preserve_metadata(&metadata, path)?;
    Ok(())
}

fn rewrite_libtool_archive(contents: &str, work_dir: &Path) -> Option<String> {
    let mut changed = false;
    let mut rewritten = String::with_capacity(contents.len());

    for line in contents.split_inclusive('\n') {
        let (body, newline) = line
            .strip_suffix('\n')
            .map(|body| (body, "\n"))
            .unwrap_or((line, ""));
        if let Some(updated) = rewrite_dependency_libs_line(body, work_dir) {
            changed = true;
            rewritten.push_str(&updated);
        } else {
            rewritten.push_str(body);
        }
        rewritten.push_str(newline);
    }

    changed.then_some(rewritten)
}

fn rewrite_dependency_libs_line(line: &str, work_dir: &Path) -> Option<String> {
    let dependencies = line
        .strip_prefix("dependency_libs='")
        .and_then(|line| line.strip_suffix('\''))?;
    let work_dir = work_dir.to_string_lossy();
    let filtered: Vec<_> = dependencies
        .split_whitespace()
        .filter(|token| !is_transient_lib_search_path(token, &work_dir))
        .collect();
    let original: Vec<_> = dependencies.split_whitespace().collect();
    if filtered == original {
        return None;
    }

    if filtered.is_empty() {
        return Some("dependency_libs=''".to_owned());
    }

    Some(format!("dependency_libs=' {}'", filtered.join(" ")))
}

fn is_transient_lib_search_path(token: &str, work_dir: &str) -> bool {
    token
        .strip_prefix("-L")
        .is_some_and(|path| Path::new(path).starts_with(work_dir))
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

    #[test]
    fn strips_transient_build_search_paths_from_libtool_archives() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("output");
        let libdir = output.join("lib");
        let work_dir = temp.path().join("work");
        fs::create_dir_all(&libdir).unwrap();
        fs::create_dir_all(&work_dir).unwrap();

        let archive = libdir.join("libbfd.la");
        fs::write(
            &archive,
            format!(
                "dependency_libs=' -L{}/source/zlib -lz /pkgs/store/example/lib/libgmp.la'\n",
                work_dir.display()
            ),
        )
        .unwrap();

        sanitize_libtool_archives(&output, &work_dir).unwrap();

        assert_eq!(
            fs::read_to_string(&archive).unwrap(),
            "dependency_libs=' -lz /pkgs/store/example/lib/libgmp.la'\n",
        );
    }
}
