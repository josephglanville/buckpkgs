use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

const STORE_PUBLICATION_PRESERVED_MODE_BITS: u32 = 0o7777 & !0o222;

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-reproducible-tree")]
pub(crate) struct Args {
    #[arg(long)]
    expected: PathBuf,

    #[arg(long)]
    actual: PathBuf,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("tree entry is missing at {path}: {side}")]
    MissingEntry { path: PathBuf, side: &'static str },

    #[error("tree entry type differs at {path}: expected {expected}, actual {actual}")]
    EntryTypeDiffers {
        path: PathBuf,
        expected: &'static str,
        actual: &'static str,
    },

    #[error("directory entries differ at {path}: expected {expected:?}, actual {actual:?}")]
    DirectoryEntriesDiffer {
        path: PathBuf,
        expected: Vec<OsString>,
        actual: Vec<OsString>,
    },

    #[error("file bytes differ at {0}")]
    FileBytesDiffer(PathBuf),

    #[error("symlink targets differ at {path}: expected {expected:?}, actual {actual:?}")]
    SymlinkTargetsDiffer {
        path: PathBuf,
        expected: PathBuf,
        actual: PathBuf,
    },

    #[error("permission modes differ at {path}: expected {expected:#o}, actual {actual:#o}")]
    PermissionModesDiffer {
        path: PathBuf,
        expected: u32,
        actual: u32,
    },

    #[error("modified times differ at {0}")]
    ModifiedTimesDiffer(PathBuf),

    #[error("unsupported filesystem entry at {0}")]
    UnsupportedEntry(PathBuf),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    compare_paths(&args.expected, &args.actual, Path::new("."))?;

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

fn compare_paths(expected: &Path, actual: &Path, relative: &Path) -> Result<(), Error> {
    let expected_metadata = metadata(expected, relative, "expected")?;
    let actual_metadata = metadata(actual, relative, "actual")?;

    let expected_kind = entry_kind(&expected_metadata);
    let actual_kind = entry_kind(&actual_metadata);
    if expected_kind != actual_kind {
        return Err(Error::EntryTypeDiffers {
            path: relative.to_path_buf(),
            expected: expected_kind,
            actual: actual_kind,
        });
    }

    compare_modes(relative, &expected_metadata, &actual_metadata)?;
    compare_modified_times(
        relative,
        expected,
        actual,
        &expected_metadata,
        &actual_metadata,
    )?;

    match expected_kind {
        "directory" => compare_directories(expected, actual, relative),
        "file" => compare_files(expected, actual, relative),
        "symlink" => compare_symlinks(expected, actual, relative),
        _ => Err(Error::UnsupportedEntry(relative.to_path_buf())),
    }
}

fn metadata(path: &Path, relative: &Path, side: &'static str) -> Result<fs::Metadata, Error> {
    fs::symlink_metadata(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            Error::MissingEntry {
                path: relative.to_path_buf(),
                side,
            }
        } else {
            common::Error::Metadata {
                path: path.to_path_buf(),
                source,
            }
            .into()
        }
    })
}

fn entry_kind(metadata: &fs::Metadata) -> &'static str {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        "directory"
    } else if file_type.is_file() {
        "file"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

fn compare_modes(
    relative: &Path,
    expected: &fs::Metadata,
    actual: &fs::Metadata,
) -> Result<(), Error> {
    let expected = expected.permissions().mode() & STORE_PUBLICATION_PRESERVED_MODE_BITS;
    let actual = actual.permissions().mode() & STORE_PUBLICATION_PRESERVED_MODE_BITS;
    if expected != actual {
        return Err(Error::PermissionModesDiffer {
            path: relative.to_path_buf(),
            expected,
            actual,
        });
    }
    Ok(())
}

fn compare_modified_times(
    relative: &Path,
    expected_path: &Path,
    actual_path: &Path,
    expected: &fs::Metadata,
    actual: &fs::Metadata,
) -> Result<(), Error> {
    let expected = expected
        .modified()
        .map_err(|source| common::Error::ModifiedTime {
            path: expected_path.to_path_buf(),
            source,
        })?;
    let actual = actual
        .modified()
        .map_err(|source| common::Error::ModifiedTime {
            path: actual_path.to_path_buf(),
            source,
        })?;
    if expected != actual {
        return Err(Error::ModifiedTimesDiffer(relative.to_path_buf()));
    }
    Ok(())
}

fn compare_directories(expected: &Path, actual: &Path, relative: &Path) -> Result<(), Error> {
    let expected_entries = child_names(expected)?;
    let actual_entries = child_names(actual)?;
    if expected_entries != actual_entries {
        return Err(Error::DirectoryEntriesDiffer {
            path: relative.to_path_buf(),
            expected: expected_entries,
            actual: actual_entries,
        });
    }

    for child in expected_entries {
        compare_paths(
            &expected.join(&child),
            &actual.join(&child),
            &relative.join(&child),
        )?;
    }

    Ok(())
}

fn child_names(path: &Path) -> Result<Vec<OsString>, Error> {
    common::sorted_dir_entries(path)?
        .into_iter()
        .map(|entry| Ok(entry.file_name()))
        .collect()
}

fn compare_files(expected: &Path, actual: &Path, relative: &Path) -> Result<(), Error> {
    let expected = fs::read(expected).map_err(|source| common::Error::ReadFile {
        path: expected.to_path_buf(),
        source,
    })?;
    let actual = fs::read(actual).map_err(|source| common::Error::ReadFile {
        path: actual.to_path_buf(),
        source,
    })?;
    if expected != actual {
        return Err(Error::FileBytesDiffer(relative.to_path_buf()));
    }
    Ok(())
}

fn compare_symlinks(expected: &Path, actual: &Path, relative: &Path) -> Result<(), Error> {
    let expected_target = fs::read_link(expected).map_err(|source| common::Error::ReadLink {
        path: expected.to_path_buf(),
        source,
    })?;
    let actual_target = fs::read_link(actual).map_err(|source| common::Error::ReadLink {
        path: actual.to_path_buf(),
        source,
    })?;
    if expected_target != actual_target {
        return Err(Error::SymlinkTargetsDiffer {
            path: relative.to_path_buf(),
            expected: expected_target,
            actual: actual_target,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn accepts_matching_trees() {
        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().join("expected");
        let actual = temp.path().join("actual");
        fs::create_dir_all(expected.join("bin")).unwrap();
        fs::create_dir_all(actual.join("bin")).unwrap();
        fs::write(expected.join("bin/tool"), "same\n").unwrap();
        fs::write(actual.join("bin/tool"), "same\n").unwrap();
        symlink("tool", expected.join("bin/current")).unwrap();
        symlink("tool", actual.join("bin/current")).unwrap();
        common::normalize_tree_mtimes(&expected).unwrap();
        common::normalize_tree_mtimes(&actual).unwrap();

        compare_paths(&expected, &actual, Path::new(".")).unwrap();
    }

    #[test]
    fn rejects_byte_mismatches() {
        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().join("expected");
        let actual = temp.path().join("actual");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&actual).unwrap();
        fs::write(expected.join("payload"), "left\n").unwrap();
        fs::write(actual.join("payload"), "right\n").unwrap();
        common::normalize_tree_mtimes(&expected).unwrap();
        common::normalize_tree_mtimes(&actual).unwrap();

        assert!(matches!(
            compare_paths(&expected, &actual, Path::new(".")),
            Err(Error::FileBytesDiffer(path)) if path == PathBuf::from("./payload")
        ));
    }

    #[test]
    fn ignores_write_bits_removed_by_store_publication() {
        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().join("expected");
        let actual = temp.path().join("actual");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&actual).unwrap();
        fs::write(expected.join("payload"), "same\n").unwrap();
        fs::write(actual.join("payload"), "same\n").unwrap();
        fs::set_permissions(&expected, fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(expected.join("payload"), fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(&actual, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(actual.join("payload"), fs::Permissions::from_mode(0o644)).unwrap();
        common::normalize_tree_mtimes(&expected).unwrap();
        common::normalize_tree_mtimes(&actual).unwrap();

        compare_paths(&expected, &actual, Path::new(".")).unwrap();
    }

    #[test]
    fn rejects_execute_bits_not_removed_by_store_publication() {
        let temp = tempfile::tempdir().unwrap();
        let expected = temp.path().join("expected");
        let actual = temp.path().join("actual");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&actual).unwrap();
        fs::write(expected.join("payload"), "same\n").unwrap();
        fs::write(actual.join("payload"), "same\n").unwrap();
        fs::set_permissions(expected.join("payload"), fs::Permissions::from_mode(0o444)).unwrap();
        fs::set_permissions(actual.join("payload"), fs::Permissions::from_mode(0o555)).unwrap();
        common::normalize_tree_mtimes(&expected).unwrap();
        common::normalize_tree_mtimes(&actual).unwrap();

        assert!(matches!(
            compare_paths(&expected, &actual, Path::new(".")),
            Err(Error::PermissionModesDiffer { path, .. }) if path == PathBuf::from("./payload")
        ));
    }
}
