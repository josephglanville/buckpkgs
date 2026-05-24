use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

const STORE_PREFIX: &[u8] = b"/pkgs/store/";

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-declared-refs")]
pub(crate) struct Args {
    #[arg(long)]
    input: Vec<PathBuf>,

    #[arg(long)]
    declared: Vec<String>,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("found undeclared store reference {reference:?} in {path}")]
    UndeclaredReference { path: PathBuf, reference: String },
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    for input in &args.input {
        verify_tree(input, &args.declared)?;
    }
    write_stamp(&args.stamp)?;
    Ok(())
}

fn verify_tree(path: &Path, declared: &[String]) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(path).map_err(|source| common::Error::Metadata {
            path: path.to_path_buf(),
            source,
        })?;
        return verify_bytes(path, target.as_os_str().as_bytes(), declared);
    }
    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            verify_tree(&entry.path(), declared)?;
        }
        return Ok(());
    }
    let bytes = fs::read(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    verify_bytes(path, &bytes, declared)
}

fn verify_bytes(path: &Path, bytes: &[u8], declared: &[String]) -> Result<(), Error> {
    let mut remaining = bytes;
    while let Some(start) = find_subslice(remaining, STORE_PREFIX) {
        let candidate = &remaining[start..];
        if !declared
            .iter()
            .any(|declared| starts_with_store_root(candidate, declared.as_bytes()))
        {
            return Err(Error::UndeclaredReference {
                path: path.to_path_buf(),
                reference: display_store_root(candidate),
            });
        }
        remaining = &candidate[STORE_PREFIX.len()..];
    }
    Ok(())
}

fn starts_with_store_root(candidate: &[u8], declared: &[u8]) -> bool {
    candidate.starts_with(declared)
        && candidate
            .get(declared.len())
            .is_none_or(|next| !is_store_entry_byte(*next))
}

fn display_store_root(candidate: &[u8]) -> String {
    let end = candidate[STORE_PREFIX.len()..]
        .iter()
        .position(|byte| !is_store_entry_byte(*byte))
        .map(|offset| STORE_PREFIX.len() + offset)
        .unwrap_or(candidate.len());
    String::from_utf8_lossy(&candidate[..end]).into_owned()
}

fn is_store_entry_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
    fn accepts_declared_file_and_symlink_references() {
        let temp = tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir_all(tree.join("lib")).unwrap();
        fs::write(
            tree.join("lib/contract.pc"),
            "libdir=/pkgs/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-contract-dev/lib",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            "/pkgs/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-contract-lib/lib/libcontract.so.1",
            tree.join("lib/libcontract.so"),
        )
        .unwrap();

        run(&Args {
            input: vec![tree],
            declared: vec![
                "/pkgs/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-contract-dev".to_owned(),
                "/pkgs/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-contract-lib".to_owned(),
            ],
            stamp: temp.path().join("stamp"),
        })
        .unwrap();
    }

    #[test]
    fn rejects_undeclared_symlink_references() {
        let temp = tempdir().unwrap();
        let tree = temp.path().join("tree");
        fs::create_dir_all(tree.join("lib")).unwrap();
        std::os::unix::fs::symlink(
            "/pkgs/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-contract-lib/lib/libcontract.so.1",
            tree.join("lib/libcontract.so"),
        )
        .unwrap();

        let result = run(&Args {
            input: vec![tree],
            declared: vec!["/pkgs/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-contract-dev".to_owned()],
            stamp: temp.path().join("stamp"),
        });
        assert!(matches!(result, Err(Error::UndeclaredReference { .. })));
    }
}
