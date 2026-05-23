#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
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
const CLOSURE_FORMAT: &str = "buckpkgs-store-closure-v1";
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct StoreClosureManifest {
    format: String,
    name: String,
    target_system: String,
    roots: Vec<String>,
    objects: BTreeMap<String, ClosureObject>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ClosureObject {
    manifest: String,
    archive: String,
    manifest_hash: String,
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

#[derive(Debug, Parser)]
#[command(name = "pkgs-export-store-closure")]
pub(crate) struct ExportClosureArgs {
    #[arg(long)]
    name: String,

    #[arg(long)]
    target_system: String,

    #[arg(long = "root")]
    roots: Vec<String>,

    #[arg(long = "object-manifest")]
    object_manifests: Vec<PathBuf>,

    #[arg(long = "object-archive")]
    object_archives: Vec<PathBuf>,

    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Parser)]
#[command(name = "pkgs-hydrate-store-closure")]
pub(crate) struct HydrateClosureArgs {
    #[arg(long)]
    closure: PathBuf,

    #[arg(long)]
    bundle: PathBuf,

    #[arg(long, default_value = "/pkgs/store")]
    store_root: PathBuf,
}

#[derive(Debug, Parser)]
#[command(name = "pkgs-project-hydrated-store-object")]
pub(crate) struct ProjectHydratedArgs {
    #[arg(long)]
    manifest: PathBuf,

    #[arg(long, default_value = "/pkgs/store")]
    store_root: PathBuf,

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

    #[arg(
        long,
        default_value = "hydrate the configured bootstrap substitute closure first"
    )]
    missing_hint: String,

    #[arg(long)]
    output: PathBuf,
}

struct ExpectedManifest<'a> {
    store_path: &'a str,
    package_name: &'a str,
    version: &'a str,
    output: &'a str,
    target_system: &'a str,
    references: &'a [String],
    runtime_store_outputs: &'a [String],
}

impl<'a> From<&'a ImportArgs> for ExpectedManifest<'a> {
    fn from(args: &'a ImportArgs) -> Self {
        Self {
            store_path: &args.expected_store_path,
            package_name: &args.expected_package_name,
            version: &args.expected_version,
            output: &args.expected_output,
            target_system: &args.expected_target_system,
            references: &args.expected_references,
            runtime_store_outputs: &args.expected_runtime_store_outputs,
        }
    }
}

impl<'a> From<&'a ProjectHydratedArgs> for ExpectedManifest<'a> {
    fn from(args: &'a ProjectHydratedArgs) -> Self {
        Self {
            store_path: &args.expected_store_path,
            package_name: &args.expected_package_name,
            version: &args.expected_version,
            output: &args.expected_output,
            target_system: &args.expected_target_system,
            references: &args.expected_references,
            runtime_store_outputs: &args.expected_runtime_store_outputs,
        }
    }
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

    #[error("failed to parse store-closure manifest {path}")]
    ParseClosureManifest {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to encode store-closure manifest")]
    EncodeClosureManifest {
        #[source]
        source: serde_json::Error,
    },

    #[error("unsupported store-object manifest format: {0}")]
    ManifestFormat(String),

    #[error("unsupported store-object archive encoding: {0}")]
    ArchiveEncoding(String),

    #[error("unsupported store-object archive compression: {0}")]
    ArchiveCompression(String),

    #[error("unsupported store-closure manifest format: {0}")]
    ClosureFormat(String),

    #[error("invalid store path key: {0}")]
    InvalidStorePathKey(String),

    #[error("store entry `{entry}` does not match key `{key}`")]
    InvalidStoreEntry { key: String, entry: String },

    #[error("store path `{actual}` does not match store entry `{entry}`")]
    InvalidStorePath { actual: String, entry: String },

    #[error("manifest contains an invalid store reference: {0}")]
    InvalidStoreReference(String),

    #[error("manifest list `{field}` is not in canonical sorted unique form")]
    NonCanonicalManifestList { field: &'static str },

    #[error("manifest runtime closure does not equal references plus its own store path")]
    InvalidManifestClosure,

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

    #[error("closure export needs one archive for each object manifest")]
    ClosureObjectArgumentCount,

    #[error("closure contains duplicate object path: {0}")]
    DuplicateClosureObject(String),

    #[error("closure root has no published object: {0}")]
    MissingClosureRoot(String),

    #[error("closure object `{object}` references unpublished object `{reference}`")]
    MissingClosureReference { object: String, reference: String },

    #[error("closure contains object not reachable from a declared root: {0}")]
    UnreachableClosureObject(String),

    #[error(
        "closure object `{object}` belongs to target system `{actual}`, expected `{expected}`"
    )]
    ClosureTargetSystem {
        object: String,
        expected: String,
        actual: String,
    },

    #[error("closure bundle path is not a safe relative path: {0:?}")]
    UnsafeBundlePath(PathBuf),

    #[error("closure manifest hash mismatch for `{object}`: expected {expected}, got {actual}")]
    ClosureManifestHash {
        object: String,
        expected: String,
        actual: String,
    },

    #[error("closure object manifest describes `{actual}`, expected `{expected}`")]
    ClosureObjectIdentity { expected: String, actual: String },

    #[error("source tree does not exist or is not a directory: {0}")]
    MissingTree(PathBuf),

    #[error("hydrated store object `{store_path}` is unavailable at {path}; {hint}")]
    MissingHydratedStoreObject {
        store_path: String,
        path: PathBuf,
        hint: String,
    },

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
    validate_expected_manifest(&manifest, ExpectedManifest::from(args))?;
    materialize_payload(&payload, &args.output)
}

pub(crate) fn export_store_closure(args: &ExportClosureArgs) -> Result<(), Error> {
    if args.object_manifests.len() != args.object_archives.len() {
        return Err(Error::ClosureObjectArgumentCount);
    }

    let mut roots = args.roots.clone();
    roots.sort();
    roots.dedup();
    let mut manifests = BTreeMap::new();
    let mut objects = BTreeMap::new();
    let mut bundle_files = Vec::new();
    for (manifest_path, archive_path) in args
        .object_manifests
        .iter()
        .zip(args.object_archives.iter())
    {
        let (manifest, _) = load_and_verify(manifest_path, archive_path)?;
        if manifest.target_system != args.target_system {
            return Err(Error::ClosureTargetSystem {
                object: manifest.store_path.clone(),
                expected: args.target_system.clone(),
                actual: manifest.target_system,
            });
        }
        let object_path = manifest.store_path.clone();
        let manifest_name = format!("{}.manifest.json", manifest.store_entry);
        let archive_name = format!("{}.bpkgs-tree", manifest.store_entry);
        let manifest_bytes = read_file(manifest_path)?;
        let manifest_hash = sha256(&manifest_bytes);
        if manifests.insert(object_path.clone(), manifest).is_some() {
            return Err(Error::DuplicateClosureObject(object_path));
        }
        objects.insert(
            object_path,
            ClosureObject {
                manifest: manifest_name.clone(),
                archive: archive_name.clone(),
                manifest_hash,
            },
        );
        bundle_files.push((manifest_path.clone(), manifest_name));
        bundle_files.push((archive_path.clone(), archive_name));
    }

    let closure = StoreClosureManifest {
        format: CLOSURE_FORMAT.to_owned(),
        name: args.name.clone(),
        target_system: args.target_system.clone(),
        roots,
        objects,
    };
    validate_closure(&closure, &manifests)?;
    if args.output.exists() {
        return Err(Error::ExistingOutput(args.output.clone()));
    }
    fs::create_dir_all(&args.output).map_err(|source| common::Error::CreateDir {
        path: args.output.clone(),
        source,
    })?;
    for (source, name) in bundle_files {
        write_file(&args.output.join(name), &read_file(&source)?)?;
    }
    let mut json = serde_json::to_vec_pretty(&closure)
        .map_err(|source| Error::EncodeClosureManifest { source })?;
    json.push(b'\n');
    write_file(&args.output.join("closure.json"), &json)
}

pub(crate) fn hydrate_store_closure(args: &HydrateClosureArgs) -> Result<(), Error> {
    let closure = load_closure_manifest(&args.closure)?;
    let mut manifests = BTreeMap::new();
    for (object_path, object) in &closure.objects {
        let manifest_path = bundle_path(&args.bundle, &object.manifest)?;
        let archive_path = bundle_path(&args.bundle, &object.archive)?;
        let manifest_bytes = read_file(&manifest_path)?;
        let actual_manifest_hash = sha256(&manifest_bytes);
        if actual_manifest_hash != object.manifest_hash {
            return Err(Error::ClosureManifestHash {
                object: object_path.clone(),
                expected: object.manifest_hash.clone(),
                actual: actual_manifest_hash,
            });
        }
        let (manifest, _) = load_and_verify(&manifest_path, &archive_path)?;
        if manifest.store_path != *object_path {
            return Err(Error::ClosureObjectIdentity {
                expected: object_path.clone(),
                actual: manifest.store_path,
            });
        }
        if manifest.target_system != closure.target_system {
            return Err(Error::ClosureTargetSystem {
                object: object_path.clone(),
                expected: closure.target_system.clone(),
                actual: manifest.target_system,
            });
        }
        manifests.insert(object_path.clone(), manifest);
    }
    validate_closure(&closure, &manifests)?;

    for object in closure.objects.values() {
        hydrate_store_object(&HydrateArgs {
            manifest: bundle_path(&args.bundle, &object.manifest)?,
            archive: bundle_path(&args.bundle, &object.archive)?,
            store_root: args.store_root.clone(),
        })?;
    }
    Ok(())
}

pub(crate) fn project_hydrated_store_object(args: &ProjectHydratedArgs) -> Result<(), Error> {
    let manifest = load_manifest(&args.manifest)?;
    validate_expected_manifest(&manifest, ExpectedManifest::from(args))?;
    let source = args.store_root.join(&manifest.store_entry);
    if !source.is_dir() {
        return Err(Error::MissingHydratedStoreObject {
            store_path: manifest.store_path,
            path: source,
            hint: args.missing_hint.clone(),
        });
    }
    verify_existing_tree(&source, &manifest.canonical_tree_hash)?;
    if args.output.exists() {
        return Err(Error::ExistingOutput(args.output.clone()));
    }
    common::copy_tree(&source, &args.output)?;
    common::normalize_tree_mtimes(&args.output)?;
    common::make_tree_read_only(&args.output)?;
    verify_existing_tree(&args.output, &manifest.canonical_tree_hash)
}

fn load_and_verify(
    manifest_path: &Path,
    archive_path: &Path,
) -> Result<(StoreObjectManifest, Vec<u8>), Error> {
    let manifest = load_manifest(manifest_path)?;

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

fn load_manifest(manifest_path: &Path) -> Result<StoreObjectManifest, Error> {
    let manifest_bytes = read_file(manifest_path)?;
    let manifest: StoreObjectManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|source| Error::ParseManifest {
            path: manifest_path.to_path_buf(),
            source,
        })?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn load_closure_manifest(path: &Path) -> Result<StoreClosureManifest, Error> {
    let bytes = read_file(path)?;
    let manifest: StoreClosureManifest =
        serde_json::from_slice(&bytes).map_err(|source| Error::ParseClosureManifest {
            path: path.to_path_buf(),
            source,
        })?;
    if manifest.format != CLOSURE_FORMAT {
        return Err(Error::ClosureFormat(manifest.format));
    }
    Ok(manifest)
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
    if canonical_store_paths(&manifest.references) != manifest.references {
        return Err(Error::NonCanonicalManifestList {
            field: "references",
        });
    }
    let mut expected_runtime_store_outputs = manifest.references.clone();
    expected_runtime_store_outputs.push(manifest.store_path.clone());
    expected_runtime_store_outputs.sort();
    expected_runtime_store_outputs.dedup();
    if manifest.runtime_store_outputs != expected_runtime_store_outputs {
        return Err(Error::InvalidManifestClosure);
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
    let prefix = format!("{key}-");
    if !entry.starts_with(&prefix)
        || entry[prefix.len()..].is_empty()
        || entry[prefix.len()..].contains('/')
    {
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

fn canonical_store_paths(paths: &[String]) -> Vec<String> {
    let mut canonical = paths.to_vec();
    canonical.sort();
    canonical.dedup();
    canonical
}

fn validate_expected_manifest(
    manifest: &StoreObjectManifest,
    expected: ExpectedManifest<'_>,
) -> Result<(), Error> {
    if manifest.store_path != expected.store_path {
        return Err(Error::UnexpectedStorePath {
            expected: expected.store_path.to_owned(),
            actual: manifest.store_path.clone(),
        });
    }
    if manifest.package.name != expected.package_name {
        return Err(Error::UnexpectedManifestMetadata {
            field: "package.name",
        });
    }
    if manifest.package.version != expected.version {
        return Err(Error::UnexpectedManifestMetadata {
            field: "package.version",
        });
    }
    if manifest.package.output != expected.output {
        return Err(Error::UnexpectedManifestMetadata {
            field: "package.output",
        });
    }
    if manifest.target_system != expected.target_system {
        return Err(Error::UnexpectedManifestMetadata {
            field: "target_system",
        });
    }
    if manifest.references != canonical_store_paths(expected.references) {
        return Err(Error::UnexpectedManifestMetadata {
            field: "references",
        });
    }
    if manifest.runtime_store_outputs != canonical_store_paths(expected.runtime_store_outputs) {
        return Err(Error::UnexpectedManifestMetadata {
            field: "runtime_store_outputs",
        });
    }
    Ok(())
}

fn validate_closure(
    closure: &StoreClosureManifest,
    manifests: &BTreeMap<String, StoreObjectManifest>,
) -> Result<(), Error> {
    if closure.format != CLOSURE_FORMAT {
        return Err(Error::ClosureFormat(closure.format.clone()));
    }
    if closure.roots != canonical_store_paths(&closure.roots) {
        return Err(Error::NonCanonicalManifestList { field: "roots" });
    }
    for root in &closure.roots {
        if !closure.objects.contains_key(root) || !manifests.contains_key(root) {
            return Err(Error::MissingClosureRoot(root.clone()));
        }
    }

    let mut reachable = BTreeSet::new();
    let mut pending = closure.roots.clone();
    while let Some(object_path) = pending.pop() {
        if !reachable.insert(object_path.clone()) {
            continue;
        }
        let manifest = manifests
            .get(&object_path)
            .ok_or_else(|| Error::MissingClosureRoot(object_path.clone()))?;
        for reference in &manifest.references {
            if !closure.objects.contains_key(reference) || !manifests.contains_key(reference) {
                return Err(Error::MissingClosureReference {
                    object: object_path.clone(),
                    reference: reference.clone(),
                });
            }
            pending.push(reference.clone());
        }
    }
    for object in closure.objects.keys() {
        if !reachable.contains(object) {
            return Err(Error::UnreachableClosureObject(object.clone()));
        }
    }
    Ok(())
}

fn bundle_path(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    let path = PathBuf::from(relative);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::UnsafeBundlePath(path));
    }
    Ok(root.join(path))
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
