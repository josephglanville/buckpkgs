#![allow(dead_code)]

use std::fs::{self, File, FileTimes};
use std::io;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("failed to inspect {path}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to create directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to canonicalize path {path}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read directory {path}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to copy file from {from} to {to}")]
    CopyFile {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read symlink {path}")]
    ReadLink {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to create symlink from {from} to {to}")]
    CreateSymlink {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to set permissions on {path}")]
    SetPermissions {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to read modified time from {path}")]
    ModifiedTime {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to set modified time on {path}")]
    SetModifiedTime {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to write stamp file {path}")]
    WriteStamp {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to remove file {path}")]
    RemoveFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to remove directory {path}")]
    RemoveDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to write file {path}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to create temporary work directory")]
    CreateTempDir {
        #[source]
        source: io::Error,
    },

    #[error("failed to compose PATH from build inputs")]
    JoinPath {
        #[source]
        source: std::env::JoinPathsError,
    },

    #[error("failed to run {program}")]
    Spawn {
        program: String,
        #[source]
        source: io::Error,
    },

    #[error("{program} failed with {status}")]
    CommandFailure { program: String, status: ExitStatus },
}

pub(crate) fn available_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|jobs| jobs.get())
        .unwrap_or(1)
}

pub(crate) fn canonicalize(path: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(path).map_err(|source| Error::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn run_command(command: &mut ProcessCommand, program: &str) -> Result<(), Error> {
    let status = command.status().map_err(|source| Error::Spawn {
        program: program.to_owned(),
        source,
    })?;
    if status.success() {
        return Ok(());
    }

    Err(Error::CommandFailure {
        program: program.to_owned(),
        status,
    })
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(source).map_err(|source_err| Error::Metadata {
        path: source.to_path_buf(),
        source: source_err,
    })?;

    if metadata.is_dir() {
        fs::create_dir(destination).map_err(|source_err| Error::CreateDir {
            path: destination.to_path_buf(),
            source: source_err,
        })?;

        let entries = fs::read_dir(source).map_err(|source_err| Error::ReadDir {
            path: source.to_path_buf(),
            source: source_err,
        })?;

        for entry in entries {
            let entry = entry.map_err(|source_err| Error::ReadDir {
                path: source.to_path_buf(),
                source: source_err,
            })?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }

        preserve_metadata(&metadata, destination)?;
        return Ok(());
    }

    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source).map_err(|source_err| Error::ReadLink {
            path: source.to_path_buf(),
            source: source_err,
        })?;
        symlink(&target, destination).map_err(|source_err| Error::CreateSymlink {
            from: target,
            to: destination.to_path_buf(),
            source: source_err,
        })?;
        return Ok(());
    }

    fs::copy(source, destination).map_err(|source_err| Error::CopyFile {
        from: source.to_path_buf(),
        to: destination.to_path_buf(),
        source: source_err,
    })?;
    preserve_metadata(&metadata, destination)
}

pub(crate) fn preserve_metadata(metadata: &fs::Metadata, destination: &Path) -> Result<(), Error> {
    fs::set_permissions(destination, metadata.permissions()).map_err(|source_err| {
        Error::SetPermissions {
            path: destination.to_path_buf(),
            source: source_err,
        }
    })?;

    let modified = metadata.modified().map_err(|source| Error::ModifiedTime {
        path: destination.to_path_buf(),
        source,
    })?;
    File::open(destination)
        .and_then(|file| file.set_times(FileTimes::new().set_modified(modified)))
        .map_err(|source| Error::SetModifiedTime {
            path: destination.to_path_buf(),
            source,
        })?;

    Ok(())
}

pub(crate) fn make_tree_read_only(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        let entries = fs::read_dir(path).map_err(|source| Error::ReadDir {
            path: path.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::ReadDir {
                path: path.to_path_buf(),
                source,
            })?;
            make_tree_read_only(&entry.path())?;
        }
    }

    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() & !0o222);
    fs::set_permissions(path, permissions).map_err(|source| Error::SetPermissions {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_at_least_one_available_job() {
        assert!(available_jobs() >= 1);
    }

    #[test]
    fn preserves_regular_file_mtime_when_copying_trees() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).unwrap();
        let file = source.join("generated");
        fs::write(&file, "generated\n").unwrap();

        let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        File::open(&file)
            .unwrap()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap();

        copy_tree(&source, &destination).unwrap();

        assert_eq!(
            fs::metadata(destination.join("generated"))
                .unwrap()
                .modified()
                .unwrap(),
            modified,
        );
    }
}
