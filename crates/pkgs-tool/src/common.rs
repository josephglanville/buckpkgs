#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, FileTimes};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};
use std::time::{Duration, UNIX_EPOCH};

use thiserror::Error;

pub(crate) const DEFAULT_MAKE_JOBS: usize = 16;

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

    #[error("failed to read file {path}")]
    ReadFile {
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

    #[error("make job count must be at least 1")]
    InvalidMakeJobs,
}

pub(crate) fn makeflags(jobs: usize) -> Result<String, Error> {
    if jobs == 0 {
        return Err(Error::InvalidMakeJobs);
    }

    Ok(format!("-j{jobs}"))
}

pub(crate) fn canonicalize(path: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(path).map_err(|source| Error::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn deterministic_scratch_dir(name: &str) -> Result<Option<PathBuf>, Error> {
    let Some(scratch_root) = std::env::var_os("BUCK_SCRATCH_PATH") else {
        return Ok(None);
    };
    let scratch_root = PathBuf::from(scratch_root);
    fs::create_dir_all(&scratch_root).map_err(|source| Error::CreateDir {
        path: scratch_root.clone(),
        source,
    })?;
    let scratch_root = canonicalize(&scratch_root)?;
    let path = scratch_root.join(name);
    if path.exists() {
        fs::remove_dir_all(&path).map_err(|source| Error::RemoveDir {
            path: path.clone(),
            source,
        })?;
    }
    fs::create_dir_all(&path).map_err(|source| Error::CreateDir {
        path: path.clone(),
        source,
    })?;
    Ok(Some(path))
}

pub(crate) fn sorted_dir_entries(path: &Path) -> Result<Vec<fs::DirEntry>, Error> {
    let entries = fs::read_dir(path).map_err(|source| Error::ReadDir {
        path: path.to_path_buf(),
        source,
    })?;
    let mut entries = entries
        .map(|entry| {
            entry.map_err(|source| Error::ReadDir {
                path: path.to_path_buf(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(entries)
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

pub(crate) fn reproducible_command(program: impl AsRef<OsStr>) -> ProcessCommand {
    let mut command = ProcessCommand::new(program);
    command
        .env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("TZ", "UTC")
        .env("SOURCE_DATE_EPOCH", "1")
        .env("PYTHONHASHSEED", "0")
        .env("PERL_HASH_SEED", "0");
    if let Some(tmpdir) = std::env::var_os("TMPDIR") {
        command.env("TMPDIR", tmpdir);
    }
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o022);
            Ok(())
        });
    }
    command
}

pub(crate) fn compiler_wrapped_path(path: &OsStr, work_dir: &Path) -> Result<OsString, Error> {
    let wrapper_dir = work_dir.join(".pkgs-compiler-wrappers");
    fs::create_dir_all(&wrapper_dir).map_err(|source| Error::CreateDir {
        path: wrapper_dir.clone(),
        source,
    })?;

    let inherited_path = path.to_string_lossy();
    let prefix_map = format!("-ffile-prefix-map={}=.", work_dir.display());
    for command in ["cc", "gcc", "c++", "g++", "cpp"] {
        let wrapper = wrapper_dir.join(command);
        let script = format!(
            "#!/bin/sh\n\
             PATH={}\n\
             export PATH\n\
             exec {} {} \"$@\"\n",
            shell_quote(&inherited_path),
            shell_quote(command),
            shell_quote(&prefix_map),
        );
        fs::write(&wrapper, script).map_err(|source| Error::WriteFile {
            path: wrapper.clone(),
            source,
        })?;

        let metadata = fs::metadata(&wrapper).map_err(|source| Error::Metadata {
            path: wrapper.clone(),
            source,
        })?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper, permissions).map_err(|source| Error::SetPermissions {
            path: wrapper,
            source,
        })?;
    }

    std::env::join_paths(std::iter::once(wrapper_dir).chain(std::env::split_paths(path)))
        .map_err(|source| Error::JoinPath { source })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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

        for entry in sorted_dir_entries(source)? {
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
        for entry in sorted_dir_entries(path)? {
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

pub(crate) fn normalize_tree_mtimes(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return normalize_symlink_mtime(path);
    }

    if metadata.is_dir() {
        for entry in sorted_dir_entries(path)? {
            normalize_tree_mtimes(&entry.path())?;
        }
    }

    let modified = UNIX_EPOCH + Duration::from_secs(1);
    File::open(path)
        .and_then(|file| file.set_times(FileTimes::new().set_modified(modified)))
        .map_err(|source| Error::SetModifiedTime {
            path: path.to_path_buf(),
            source,
        })
}

fn normalize_symlink_mtime(path: &Path) -> Result<(), Error> {
    let path_c = std::ffi::CString::new(path.as_os_str().as_bytes())
        .expect("filesystem paths cannot contain NUL bytes");
    let normalized = libc::timespec {
        tv_sec: 1,
        tv_nsec: 0,
    };
    let times = [normalized, normalized];
    let result = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            path_c.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        return Ok(());
    }

    Err(Error::SetModifiedTime {
        path: path.to_path_buf(),
        source: io::Error::last_os_error(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static UMASK_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct UmaskGuard(libc::mode_t);

    impl UmaskGuard {
        fn set(mask: libc::mode_t) -> Self {
            Self(unsafe { libc::umask(mask) })
        }
    }

    impl Drop for UmaskGuard {
        fn drop(&mut self) {
            unsafe {
                libc::umask(self.0);
            }
        }
    }

    #[test]
    fn formats_explicit_make_job_counts() {
        assert_eq!(makeflags(DEFAULT_MAKE_JOBS).unwrap(), "-j16");
        assert!(makeflags(0).is_err());
    }

    #[test]
    fn sorts_directory_entries_by_name() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("zeta"), "z\n").unwrap();
        fs::write(temp.path().join("alpha"), "a\n").unwrap();
        fs::write(temp.path().join("middle"), "m\n").unwrap();

        let names = sorted_dir_entries(temp.path())
            .unwrap()
            .into_iter()
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                std::ffi::OsString::from("alpha"),
                std::ffi::OsString::from("middle"),
                std::ffi::OsString::from("zeta"),
            ],
        );
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

    #[test]
    fn writes_compiler_wrappers_without_mutating_package_flags() {
        let temp = tempfile::tempdir().unwrap();
        let work_dir = temp.path().join("work");
        fs::create_dir(&work_dir).unwrap();
        let path = std::env::join_paths(["/seed/bin", "/tools/bin"]).unwrap();

        let wrapped = compiler_wrapped_path(&path, &work_dir).unwrap();
        let wrapped_entries: Vec<_> = std::env::split_paths(&wrapped).collect();
        assert_eq!(wrapped_entries[0], work_dir.join(".pkgs-compiler-wrappers"));
        assert_eq!(wrapped_entries[1], PathBuf::from("/seed/bin"));
        assert_eq!(wrapped_entries[2], PathBuf::from("/tools/bin"));

        let cc = fs::read_to_string(work_dir.join(".pkgs-compiler-wrappers/cc")).unwrap();
        assert!(cc.contains("PATH='/seed/bin:/tools/bin'"));
        assert!(cc.contains(&format!(
            "exec 'cc' '-ffile-prefix-map={}=.' \"$@\"",
            work_dir.display()
        )));
    }

    #[test]
    fn normalizes_tree_mtimes_to_the_reproducible_epoch() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("tree");
        let child = root.join("artifact");
        let link = root.join("artifact-link");
        fs::create_dir(&root).unwrap();
        fs::write(&child, "artifact\n").unwrap();
        symlink("artifact", &link).unwrap();

        normalize_tree_mtimes(&root).unwrap();

        let expected = UNIX_EPOCH + Duration::from_secs(1);
        assert_eq!(fs::metadata(&child).unwrap().modified().unwrap(), expected);
        assert_eq!(fs::metadata(&root).unwrap().modified().unwrap(), expected);
        assert_eq!(
            fs::symlink_metadata(&link).unwrap().modified().unwrap(),
            expected,
        );
    }

    #[test]
    fn pins_child_process_umask_for_created_files() {
        let _umask_lock = UMASK_TEST_LOCK.lock().unwrap();
        let _umask = UmaskGuard::set(0o077);
        let temp = tempfile::tempdir().unwrap();
        let mut command = reproducible_command("sh");
        command
            .current_dir(temp.path())
            .arg("-c")
            .arg("touch artifact");
        run_command(&mut command, "sh").unwrap();

        let mode = fs::metadata(temp.path().join("artifact"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o644);
    }
}
