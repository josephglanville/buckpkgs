use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-no-refs")]
pub(crate) struct Args {
    #[arg(long)]
    input: Vec<PathBuf>,

    #[arg(long)]
    forbidden: Vec<String>,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("found forbidden reference {reference:?} in {path}")]
    ForbiddenReference { path: PathBuf, reference: String },
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    for input in &args.input {
        scan_forbidden_refs(input, &args.forbidden)?;
    }

    if let Some(parent) = args.stamp.parent() {
        fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    fs::write(&args.stamp, "ok\n").map_err(|source| common::Error::WriteStamp {
        path: args.stamp.clone(),
        source,
    })?;
    Ok(())
}

fn scan_forbidden_refs(path: &Path, forbidden: &[String]) -> Result<(), Error> {
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
            scan_forbidden_refs(&entry.path(), forbidden)?;
        }
        return Ok(());
    }

    let bytes = fs::read(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(reference) = find_forbidden_ref(&bytes, forbidden) {
        return Err(Error::ForbiddenReference {
            path: path.to_path_buf(),
            reference: reference.to_owned(),
        });
    }

    Ok(())
}

fn find_forbidden_ref<'a>(haystack: &[u8], forbidden: &'a [String]) -> Option<&'a str> {
    let common_prefix = common_prefix(forbidden);

    if common_prefix.is_empty() {
        return forbidden
            .iter()
            .find(|reference| contains_subslice(haystack, reference.as_bytes()))
            .map(String::as_str);
    }

    haystack
        .windows(common_prefix.len())
        .enumerate()
        .filter(|(_, window)| *window == common_prefix)
        .find_map(|(start, _)| {
            forbidden
                .iter()
                .find(|reference| haystack[start..].starts_with(reference.as_bytes()))
                .map(String::as_str)
        })
}

fn common_prefix(forbidden: &[String]) -> &[u8] {
    let Some(first) = forbidden.first() else {
        return &[];
    };

    let first = first.as_bytes();
    let mut prefix_len = first.len();

    for reference in forbidden.iter().skip(1) {
        prefix_len = first
            .iter()
            .take(prefix_len)
            .zip(reference.as_bytes())
            .take_while(|(left, right)| left == right)
            .count();
    }

    &first[..prefix_len]
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_subslices() {
        assert!(contains_subslice(
            b"prefix /pkgs/store/abc suffix",
            b"/pkgs/store/abc"
        ));
        assert!(!contains_subslice(b"prefix suffix", b"/pkgs/store/abc"));
        assert!(!contains_subslice(b"prefix suffix", b""));
    }

    #[test]
    fn finds_forbidden_refs_after_scanning_the_shared_prefix_once() {
        let forbidden = vec![
            "/pkgs/store/aaa-foreign".to_owned(),
            "/pkgs/store/bbb-foreign".to_owned(),
        ];

        assert_eq!(
            find_forbidden_ref(b"prefix /pkgs/store/bbb-foreign suffix", &forbidden),
            Some("/pkgs/store/bbb-foreign"),
        );
        assert_eq!(find_forbidden_ref(b"prefix suffix", &forbidden), None);
        assert_eq!(common_prefix(&forbidden), b"/pkgs/store/");
    }

    #[test]
    fn falls_back_when_refs_do_not_share_a_prefix() {
        let forbidden = vec!["abc".to_owned(), "xyz".to_owned()];

        assert_eq!(common_prefix(&forbidden), b"");
        assert_eq!(find_forbidden_ref(b"---xyz---", &forbidden), Some("xyz"));
    }
}
