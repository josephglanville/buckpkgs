#![allow(dead_code)]

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::common;

const MANIFEST_FORMAT: &str = "buckpkgs-store-object-v1";
const ARCHIVE_ENCODING: &str = "buckpkgs-tree-v1";
const ARCHIVE_COMPRESSION: &str = "none";
const ARCHIVE_MAGIC: &[u8] = b"BUCKPKGS-STORE-ARCHIVE-V1\0";
const RECORD_END: u8 = 0;
const RECORD_DIRECTORY: u8 = 1;
const RECORD_FILE: u8 = 2;
const RECORD_SYMLINK: u8 = 3;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoreObjectManifest {
    format: String,
    store_path: String,
    store_path_key: String,
    store_entry: String,
    package: PackageIdentity,
    target_system: String,
    archive: ArchiveIdentity,
    canonical_tree_hash: String,
    references: Vec<String>,
    runtime_store_outputs: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct PackageIdentity {
    name: String,
    version: String,
    output: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ArchiveIdentity {
    encoding: String,
    compression: String,
    download_hash: String,
    download_size: u64,
    payload_hash: String,
    payload_size: u64,
}

#[derive(Debug, Parser)]
#[command(name = "pkgs-export-store-object")]
pub(crate) struct ExportArgs {
    #[arg(long)]
    input: PathBuf,

    #[arg(long)]
    store_path: String,

    #[arg(long)]
    store_path_key: String,

    #[arg(long)]
    store_entry: String,

    #[arg(long)]
    package_name: String,

    #[arg(long)]
    version: String,

    #[arg(long, default_value = "out")]
    output: String,

    #[arg(long)]
    target_system: String,

    #[arg(long = "reference")]
    references: Vec<String>,

    #[arg(long = "runtime-store-output")]
    runtime_store_outputs: Vec<String>,

    #[arg(long)]
    archive: PathBuf,

    #[arg(long)]
    manifest: PathBuf,
}

#[derive(Debug, Parser)]
#[command(name = "pkgs-hydrate-store-object")]
pub(crate) struct HydrateArgs {
    #[arg(long)]
    manifest: PathBuf,

    #[arg(long)]
    archive: PathBuf,

    #[arg(long, default_value = "/pkgs/store")]
    store_root: PathBuf,
}

#[derive(Debug, Parser)]
#[command(name = "pkgs-import-store-object")]
pub(crate) struct ImportArgs {
    #[arg(long)]
    manifest: PathBuf,

    #[arg(long)]
    archive: PathBuf,

    #[arg(long)]
    expected_store_path: String,

    #[arg(long)]
    expected_package_name: String,

    #[arg(long)]
    expected_version: String,

    #[arg(long, default_value = "out")]
    expected_output: String,

    #[arg(long)]
    expected_target_system: String,

    #[arg(long = "expected-reference")]
    expected_references: Vec<String>,

    #[arg(long = "expected-runtime-store-output")]
    expected_runtime_store_outputs: Vec<String>,

    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("failed to read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse store-object manifest {path}")]
    ParseManifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to encode store-object manifest")]
    EncodeManifest {
        #[source]
        source: serde_json::Error,
    },

    #[error("unsupported store-object manifest format: {0}")]
    ManifestFormat(String),

    #[error("unsupported store-object archive encoding: {0}")]
    ArchiveEncoding(String),

    #[error("unsupported store-object archive compression: {0}")]
    ArchiveCompression(String),

    #[error("invalid store path key: {0}")]
    InvalidStorePathKey(String),

    #[error("store entry `{entry}` does not match key `{key}`")]
    InvalidStoreEntry { key: String, entry: String },

    #[error("store path `{actual}` does not match store entry `{entry}`")]
    InvalidStorePath { actual: String, entry: String },

    #[error("manifest contains an invalid store reference: {0}")]
    InvalidStoreReference(String),

    #[error("manifest store path `{actual}` does not match expected path `{expected}`")]
    UnexpectedStorePath { expected: String, actual: String },

    #[error("manifest metadata field `{field}` does not match expected value")]
    UnexpectedManifestMetadata { field: &'static str },

    #[error("archive hash mismatch: expected {expected}, got {actual}")]
    ArchiveHash { expected: String, actual: String },

    #[error("archive size mismatch: expected {expected}, got {actual}")]
    ArchiveSize { expected: u64, actual: u64 },

    #[error("canonical tree hash mismatch: expected {expected}, got {actual}")]
    TreeHash { expected: String, actual: String },

    #[error("source tree does not exist or is not a directory: {0}")]
    MissingTree(PathBuf),

    #[error("output path already exists: {0}")]
    ExistingOutput(PathBuf),

    #[error("unsupported filesystem entry in store tree: {0}")]
    UnsupportedEntry(PathBuf),

    #[error("archive is malformed: {0}")]
    MalformedArchive(&'static str),

    #[error("archive entry is not a safe relative path: {0:?}")]
    UnsafeArchivePath(PathBuf),

    #[error("archive contains a duplicate entry: {0:?}")]
    DuplicateArchiveEntry(PathBuf),

    #[error("archive contains an entry outside its declared directory tree: {0:?}")]
    InvalidArchiveLayout(PathBuf),

    #[error("archive entry is not in canonical traversal order: {0:?}")]
    NonCanonicalArchiveOrder(PathBuf),

    #[error("failed to acquire store lock {path}")]
    StoreLock {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to rename {from} to {to}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) fn export_store_object(args: &ExportArgs) -> Result<(), Error> {
    validate_identity(&args.store_path_key, &args.store_entry, &args.store_path)?;
    if !args.input.is_dir() {
        return Err(Error::MissingTree(args.input.clone()));
    }

    let payload = encode_tree(&args.input)?;
    let digest = sha256(&payload);
    let size = payload.len() as u64;
    let mut references = args.references.clone();
    references.sort();
    references.dedup();
    let mut runtime_store_outputs = args.runtime_store_outputs.clone();
    runtime_store_outputs.sort();
    runtime_store_outputs.dedup();

    let manifest = StoreObjectManifest {
        format: MANIFEST_FORMAT.to_owned(),
        store_path: args.store_path.clone(),
        store_path_key: args.store_path_key.clone(),
        store_entry: args.store_entry.clone(),
        package: PackageIdentity {
            name: args.package_name.clone(),
            version: args.version.clone(),
            output: args.output.clone(),
        },
        target_system: args.target_system.clone(),
        archive: ArchiveIdentity {
            encoding: ARCHIVE_ENCODING.to_owned(),
            compression: ARCHIVE_COMPRESSION.to_owned(),
            download_hash: digest.clone(),
            download_size: size,
            payload_hash: digest.clone(),
            payload_size: size,
        },
        canonical_tree_hash: digest,
        references,
        runtime_store_outputs,
    };
    validate_manifest(&manifest)?;

    write_file(&args.archive, &payload)?;
    let mut json =
        serde_json::to_vec_pretty(&manifest).map_err(|source| Error::EncodeManifest { source })?;
    json.push(b'\n');
    write_file(&args.manifest, &json)
}

pub(crate) fn hydrate_store_object(args: &HydrateArgs) -> Result<PathBuf, Error> {
    let (manifest, payload) = load_and_verify(&args.manifest, &args.archive)?;
    fs::create_dir_all(&args.store_root).map_err(|source| common::Error::CreateDir {
        path: args.store_root.clone(),
        source,
    })?;
    let destination = args.store_root.join(&manifest.store_entry);
    let lock_path = args
        .store_root
        .join(format!("{}.buckpkgs.lock", manifest.store_entry));
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| Error::StoreLock {
            path: lock_path.clone(),
            source,
        })?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(Error::StoreLock {
            path: lock_path,
            source: std::io::Error::last_os_error(),
        });
    }

    if destination.exists() {
        verify_existing_tree(&destination, &manifest.canonical_tree_hash)?;
        return Ok(destination);
    }

    let temporary = args
        .store_root
        .join(format!("{}.buckpkgs.tmp", manifest.store_entry));
    if temporary.exists() {
        fs::remove_dir_all(&temporary).map_err(|source| common::Error::RemoveDir {
            path: temporary.clone(),
            source,
        })?;
    }
    if let Err(err) = materialize_payload(&payload, &temporary) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(err);
    }
    if let Err(source) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_dir_all(&temporary);
        return Err(Error::Rename {
            from: temporary,
            to: destination.clone(),
            source,
        });
    }
    Ok(destination)
}

pub(crate) fn import_store_object(args: &ImportArgs) -> Result<(), Error> {
    let (manifest, payload) = load_and_verify(&args.manifest, &args.archive)?;
    if manifest.store_path != args.expected_store_path {
        return Err(Error::UnexpectedStorePath {
            expected: args.expected_store_path.clone(),
            actual: manifest.store_path,
        });
    }
    if manifest.package.name != args.expected_package_name {
        return Err(Error::UnexpectedManifestMetadata {
            field: "package.name",
        });
    }
    if manifest.package.version != args.expected_version {
        return Err(Error::UnexpectedManifestMetadata {
            field: "package.version",
        });
    }
    if manifest.package.output != args.expected_output {
        return Err(Error::UnexpectedManifestMetadata {
            field: "package.output",
        });
    }
    if manifest.target_system != args.expected_target_system {
        return Err(Error::UnexpectedManifestMetadata {
            field: "target_system",
        });
    }
    let mut expected_references = args.expected_references.clone();
    expected_references.sort();
    expected_references.dedup();
    if manifest.references != expected_references {
        return Err(Error::UnexpectedManifestMetadata {
            field: "references",
        });
    }
    let mut expected_runtime_store_outputs = args.expected_runtime_store_outputs.clone();
    expected_runtime_store_outputs.sort();
    expected_runtime_store_outputs.dedup();
    if manifest.runtime_store_outputs != expected_runtime_store_outputs {
        return Err(Error::UnexpectedManifestMetadata {
            field: "runtime_store_outputs",
        });
    }
    materialize_payload(&payload, &args.output)
}

fn load_and_verify(
    manifest_path: &Path,
    archive_path: &Path,
) -> Result<(StoreObjectManifest, Vec<u8>), Error> {
    let manifest_bytes = read_file(manifest_path)?;
    let manifest: StoreObjectManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|source| Error::ParseManifest {
            path: manifest_path.to_path_buf(),
            source,
        })?;
    validate_manifest(&manifest)?;

    let payload = read_file(archive_path)?;
    let digest = sha256(&payload);
    if digest != manifest.archive.download_hash || digest != manifest.archive.payload_hash {
        return Err(Error::ArchiveHash {
            expected: manifest.archive.payload_hash.clone(),
            actual: digest,
        });
    }
    let size = payload.len() as u64;
    if size != manifest.archive.download_size || size != manifest.archive.payload_size {
        return Err(Error::ArchiveSize {
            expected: manifest.archive.payload_size,
            actual: size,
        });
    }
    if manifest.canonical_tree_hash != manifest.archive.payload_hash {
        return Err(Error::TreeHash {
            expected: manifest.canonical_tree_hash.clone(),
            actual: manifest.archive.payload_hash.clone(),
        });
    }
    validate_payload(&payload)?;
    Ok((manifest, payload))
}

fn validate_manifest(manifest: &StoreObjectManifest) -> Result<(), Error> {
    if manifest.format != MANIFEST_FORMAT {
        return Err(Error::ManifestFormat(manifest.format.clone()));
    }
    if manifest.archive.encoding != ARCHIVE_ENCODING {
        return Err(Error::ArchiveEncoding(manifest.archive.encoding.clone()));
    }
    if manifest.archive.compression != ARCHIVE_COMPRESSION {
        return Err(Error::ArchiveCompression(
            manifest.archive.compression.clone(),
        ));
    }
    validate_identity(
        &manifest.store_path_key,
        &manifest.store_entry,
        &manifest.store_path,
    )?;
    for reference in manifest
        .references
        .iter()
        .chain(manifest.runtime_store_outputs.iter())
    {
        validate_store_reference(reference)?;
    }
    Ok(())
}

fn validate_identity(key: &str, entry: &str, store_path: &str) -> Result<(), Error> {
    if key.len() != 32
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::InvalidStorePathKey(key.to_owned()));
    }
    if !entry.starts_with(&format!("{key}-")) {
        return Err(Error::InvalidStoreEntry {
            key: key.to_owned(),
            entry: entry.to_owned(),
        });
    }
    let expected = format!("/pkgs/store/{entry}");
    if store_path != expected {
        return Err(Error::InvalidStorePath {
            actual: store_path.to_owned(),
            entry: entry.to_owned(),
        });
    }
    Ok(())
}

fn validate_store_reference(reference: &str) -> Result<(), Error> {
    let Some(entry) = reference.strip_prefix("/pkgs/store/") else {
        return Err(Error::InvalidStoreReference(reference.to_owned()));
    };
    if entry.is_empty() || entry.contains('/') {
        return Err(Error::InvalidStoreReference(reference.to_owned()));
    }
    Ok(())
}

fn verify_existing_tree(path: &Path, expected_hash: &str) -> Result<(), Error> {
    let actual = sha256(&encode_tree(path)?);
    if actual == expected_hash {
        Ok(())
    } else {
        Err(Error::TreeHash {
            expected: expected_hash.to_owned(),
            actual,
        })
    }
}

fn encode_tree(root: &Path) -> Result<Vec<u8>, Error> {
    let mut bytes = ARCHIVE_MAGIC.to_vec();
    encode_entry(root, Path::new(""), &mut bytes)?;
    bytes.push(RECORD_END);
    Ok(bytes)
}

fn encode_entry(path: &Path, relative: &Path, output: &mut Vec<u8>) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_dir() {
        write_path_record(output, RECORD_DIRECTORY, relative)?;
        for entry in common::sorted_dir_entries(path)? {
            encode_entry(&entry.path(), &relative.join(entry.file_name()), output)?;
        }
        return Ok(());
    }
    if metadata.file_type().is_symlink() {
        write_path_record(output, RECORD_SYMLINK, relative)?;
        let target = fs::read_link(path).map_err(|source| common::Error::ReadLink {
            path: path.to_path_buf(),
            source,
        })?;
        write_bytes(output, target.as_os_str().as_bytes())?;
        return Ok(());
    }
    if metadata.is_file() {
        write_path_record(output, RECORD_FILE, relative)?;
        output.push(u8::from(metadata.permissions().mode() & 0o111 != 0));
        let bytes = read_file(path)?;
        write_u64(output, bytes.len() as u64);
        output.extend_from_slice(&bytes);
        return Ok(());
    }
    Err(Error::UnsupportedEntry(path.to_path_buf()))
}

fn materialize_payload(payload: &[u8], output: &Path) -> Result<(), Error> {
    validate_payload(payload)?;
    if output.exists() {
        return Err(Error::ExistingOutput(output.to_path_buf()));
    }
    let mut cursor = payload_cursor(payload)?;
    let mut seen = BTreeSet::new();
    loop {
        let kind = read_u8(&mut cursor)?;
        if kind == RECORD_END {
            if cursor.position() != payload.len() as u64 {
                return Err(Error::MalformedArchive("bytes after end marker"));
            }
            break;
        }
        let relative = read_path(&mut cursor)?;
        validate_unique_entry(&relative, &mut seen)?;
        let destination = output.join(&relative);
        match kind {
            RECORD_DIRECTORY => {
                fs::create_dir_all(&destination).map_err(|source| common::Error::CreateDir {
                    path: destination,
                    source,
                })?;
            }
            RECORD_FILE => {
                let executable = read_u8(&mut cursor)?;
                if executable > 1 {
                    return Err(Error::MalformedArchive("invalid executable marker"));
                }
                let contents = read_file_contents(&mut cursor)?;
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                fs::write(&destination, contents).map_err(|source| Error::Write {
                    path: destination.clone(),
                    source,
                })?;
                if executable == 1 {
                    let mut permissions = fs::metadata(&destination)
                        .map_err(|source| common::Error::Metadata {
                            path: destination.clone(),
                            source,
                        })?
                        .permissions();
                    permissions.set_mode(permissions.mode() | 0o111);
                    fs::set_permissions(&destination, permissions).map_err(|source| {
                        common::Error::SetPermissions {
                            path: destination,
                            source,
                        }
                    })?;
                }
            }
            RECORD_SYMLINK => {
                let target = OsString::from_vec(read_bytes(&mut cursor)?);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                symlink(&target, &destination).map_err(|source| common::Error::CreateSymlink {
                    from: PathBuf::from(target),
                    to: destination,
                    source,
                })?;
            }
            _ => return Err(Error::MalformedArchive("unknown record type")),
        }
    }
    common::normalize_tree_mtimes(output)?;
    common::make_tree_read_only(output)?;
    Ok(())
}

fn validate_payload(payload: &[u8]) -> Result<(), Error> {
    let mut cursor = payload_cursor(payload)?;
    let mut seen = BTreeSet::new();
    let mut open_directories: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    loop {
        let kind = read_u8(&mut cursor)?;
        if kind == RECORD_END {
            if cursor.position() != payload.len() as u64 {
                return Err(Error::MalformedArchive("bytes after end marker"));
            }
            return Ok(());
        }
        let relative = read_path(&mut cursor)?;
        validate_unique_entry(&relative, &mut seen)?;
        if relative.as_os_str().is_empty() {
            if kind != RECORD_DIRECTORY || !open_directories.is_empty() {
                return Err(Error::InvalidArchiveLayout(relative));
            }
            open_directories.push((relative.clone(), None));
        } else {
            let parent = relative
                .parent()
                .ok_or_else(|| Error::InvalidArchiveLayout(relative.clone()))?;
            while open_directories
                .last()
                .is_some_and(|(directory, _)| directory.as_path() != parent)
            {
                open_directories.pop();
            }
            let Some((_, last_child)) = open_directories.last_mut() else {
                return Err(Error::InvalidArchiveLayout(relative));
            };
            let child = relative
                .file_name()
                .ok_or_else(|| Error::InvalidArchiveLayout(relative.clone()))?
                .as_bytes()
                .to_vec();
            if last_child
                .as_ref()
                .is_some_and(|previous| child.as_slice() <= previous.as_slice())
            {
                return Err(Error::NonCanonicalArchiveOrder(relative));
            }
            *last_child = Some(child);
        }
        match kind {
            RECORD_DIRECTORY => {
                if !relative.as_os_str().is_empty() {
                    open_directories.push((relative, None));
                }
            }
            RECORD_FILE => {
                if read_u8(&mut cursor)? > 1 {
                    return Err(Error::MalformedArchive("invalid executable marker"));
                }
                let _ = read_file_contents(&mut cursor)?;
            }
            RECORD_SYMLINK => {
                let _ = read_bytes(&mut cursor)?;
            }
            _ => return Err(Error::MalformedArchive("unknown record type")),
        }
    }
}

fn validate_unique_entry(path: &Path, seen: &mut BTreeSet<Vec<u8>>) -> Result<(), Error> {
    let bytes = path.as_os_str().as_bytes().to_vec();
    if !seen.insert(bytes) {
        return Err(Error::DuplicateArchiveEntry(path.to_path_buf()));
    }
    Ok(())
}

fn write_path_record(output: &mut Vec<u8>, kind: u8, path: &Path) -> Result<(), Error> {
    output.push(kind);
    write_bytes(output, path.as_os_str().as_bytes())
}

fn write_bytes(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    let len: u32 = bytes
        .len()
        .try_into()
        .map_err(|_| Error::MalformedArchive("entry exceeds archive length limit"))?;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn write_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn payload_cursor(payload: &[u8]) -> Result<Cursor<&[u8]>, Error> {
    if !payload.starts_with(ARCHIVE_MAGIC) {
        return Err(Error::MalformedArchive("missing archive magic"));
    }
    let mut cursor = Cursor::new(payload);
    cursor.set_position(ARCHIVE_MAGIC.len() as u64);
    Ok(cursor)
}

fn read_u8(cursor: &mut Cursor<&[u8]>) -> Result<u8, Error> {
    let mut bytes = [0];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| Error::MalformedArchive("unexpected end of archive"))?;
    Ok(bytes[0])
}

fn read_path(cursor: &mut Cursor<&[u8]>) -> Result<PathBuf, Error> {
    let path = PathBuf::from(OsString::from_vec(read_bytes(cursor)?));
    if path.is_absolute()
        || (!path.as_os_str().is_empty()
            && path
                .components()
                .any(|component| !matches!(component, Component::Normal(_))))
    {
        return Err(Error::UnsafeArchivePath(path));
    }
    Ok(path)
}

fn read_bytes(cursor: &mut Cursor<&[u8]>) -> Result<Vec<u8>, Error> {
    let mut len = [0; 4];
    cursor
        .read_exact(&mut len)
        .map_err(|_| Error::MalformedArchive("truncated length"))?;
    let len = u32::from_be_bytes(len) as usize;
    let mut bytes = vec![0; len];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| Error::MalformedArchive("truncated contents"))?;
    Ok(bytes)
}

fn read_file_contents(cursor: &mut Cursor<&[u8]>) -> Result<Vec<u8>, Error> {
    let mut len = [0; 8];
    cursor
        .read_exact(&mut len)
        .map_err(|_| Error::MalformedArchive("truncated file length"))?;
    let len: usize = u64::from_be_bytes(len)
        .try_into()
        .map_err(|_| Error::MalformedArchive("file length exceeds platform limit"))?;
    let mut bytes = vec![0; len];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| Error::MalformedArchive("truncated file contents"))?;
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity("sha256:".len() + digest.len() * 2);
    value.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").expect("writing to string cannot fail");
    }
    value
}

fn read_file(path: &Path) -> Result<Vec<u8>, Error> {
    fs::read(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| common::Error::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut file = File::create(path).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })?;
    file.write_all(contents).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })
}
