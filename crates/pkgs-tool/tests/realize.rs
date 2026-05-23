use std::fs::{self, File, FileTimes};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

#[test]
fn composes_multiple_source_trees_without_clobbering() {
    let temp = tempdir().unwrap();
    let base = temp.path().join("base");
    let extra = temp.path().join("extra");
    fs::create_dir_all(base.join("src")).unwrap();
    fs::create_dir_all(extra.join("vendor")).unwrap();
    fs::write(base.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();
    fs::write(extra.join("vendor/README"), "vendored payload\n").unwrap();

    let output = temp.path().join("composed");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_compose_sources"))
        .args([
            "--source",
            base.to_str().unwrap(),
            "--source",
            extra.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(output.join("src/main.c")).unwrap(),
        "int main(void) { return 0; }\n"
    );
    assert_eq!(
        fs::read_to_string(output.join("vendor/README")).unwrap(),
        "vendored payload\n"
    );
}

#[test]
fn rejects_conflicting_composed_sources() {
    let temp = tempdir().unwrap();
    let base = temp.path().join("base");
    let extra = temp.path().join("extra");
    fs::create_dir_all(&base).unwrap();
    fs::create_dir_all(&extra).unwrap();
    fs::write(base.join("README"), "first\n").unwrap();
    fs::write(extra.join("README"), "second\n").unwrap();

    let output = temp.path().join("composed");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_compose_sources"))
        .args([
            "--source",
            base.to_str().unwrap(),
            "--source",
            extra.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
}

#[test]
fn stages_an_immutable_tree_once() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    let bin = source.join("bin");
    fs::create_dir_all(&bin).unwrap();

    let bash = bin.join("bash");
    fs::write(&bash, "#!/bin/sh\necho bash\n").unwrap();
    let mut permissions = fs::metadata(&bash).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&bash, permissions).unwrap();

    let output = temp.path().join("output");

    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_stage_tree"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let realized = output.join("bin/bash");
    assert_eq!(
        fs::read_to_string(&realized).unwrap(),
        "#!/bin/sh\necho bash\n"
    );
    assert_eq!(
        fs::metadata(&realized).unwrap().permissions().mode() & 0o222,
        0
    );
    fs::write(&bash, "changed\n").unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_stage_tree"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(realized).unwrap(),
        "#!/bin/sh\necho bash\n"
    );
}

#[test]
fn stages_an_immutable_tree() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    let bin = source.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("tool"), "tool\n").unwrap();

    let staged_output = temp.path().join("staged-output");

    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_stage_tree"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            staged_output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    assert_eq!(
        fs::read_to_string(staged_output.join("bin/tool")).unwrap(),
        "tool\n"
    );
    assert_eq!(
        fs::metadata(staged_output.join("bin/tool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o222,
        0
    );
}

#[test]
fn exports_hydrates_and_imports_store_objects() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(source.join("bin")).unwrap();
    fs::write(source.join("bin/tool"), "#!/bin/sh\necho imported\n").unwrap();
    let mut permissions = fs::metadata(source.join("bin/tool")).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(source.join("bin/tool"), permissions).unwrap();
    symlink("tool", source.join("bin/tool-link")).unwrap();

    let key = "0123456789abcdef0123456789abcdef";
    let entry = format!("{key}-example-1.0");
    let store_path = format!("/pkgs/store/{entry}");
    let archive = temp.path().join("example.bpkgs-tree");
    let manifest = temp.path().join("example.manifest.json");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_object"))
        .args([
            "--input",
            source.to_str().unwrap(),
            "--store-path",
            &store_path,
            "--store-path-key",
            key,
            "--store-entry",
            &entry,
            "--package-name",
            "example",
            "--version",
            "1.0",
            "--target-system",
            "x86_64-linux",
            "--runtime-store-output",
            &store_path,
            "--archive",
            archive.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let manifest_contents = fs::read_to_string(&manifest).unwrap();
    assert!(manifest_contents.contains("\"format\": \"buckpkgs-store-object-v1\""));
    assert!(manifest_contents.contains("\"encoding\": \"buckpkgs-tree-v1\""));
    assert!(manifest_contents.contains(&store_path));

    let store_root = temp.path().join("store");
    for _ in 0..2 {
        let status = Command::new(env!("CARGO_BIN_EXE_pkgs_hydrate_store_object"))
            .args([
                "--manifest",
                manifest.to_str().unwrap(),
                "--archive",
                archive.to_str().unwrap(),
                "--store-root",
                store_root.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    let hydrated = store_root.join(&entry);
    assert_eq!(
        fs::read_to_string(hydrated.join("bin/tool")).unwrap(),
        "#!/bin/sh\necho imported\n"
    );
    assert_eq!(
        fs::read_link(hydrated.join("bin/tool-link")).unwrap(),
        Path::new("tool")
    );
    assert_eq!(
        fs::metadata(hydrated.join("bin/tool"))
            .unwrap()
            .permissions()
            .mode()
            & 0o222,
        0
    );

    let imported = temp.path().join("imported");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_import_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--archive",
            archive.to_str().unwrap(),
            "--expected-store-path",
            &store_path,
            "--expected-package-name",
            "example",
            "--expected-version",
            "1.0",
            "--expected-target-system",
            "x86_64-linux",
            "--expected-runtime-store-output",
            &store_path,
            "--output",
            imported.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(imported.join("bin/tool")).unwrap(),
        "#!/bin/sh\necho imported\n"
    );
}

#[test]
fn rejects_tampered_store_object_archives() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), "payload\n").unwrap();
    let key = "0123456789abcdef0123456789abcdef";
    let entry = format!("{key}-example-1.0");
    let store_path = format!("/pkgs/store/{entry}");
    let archive = temp.path().join("example.bpkgs-tree");
    let manifest = temp.path().join("example.manifest.json");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_object"))
        .args([
            "--input",
            source.to_str().unwrap(),
            "--store-path",
            &store_path,
            "--store-path-key",
            key,
            "--store-entry",
            &entry,
            "--package-name",
            "example",
            "--version",
            "1.0",
            "--target-system",
            "x86_64-linux",
            "--runtime-store-output",
            &store_path,
            "--archive",
            archive.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let mut tampered = fs::read(&archive).unwrap();
    tampered.extend_from_slice(b"changed");
    fs::write(&archive, tampered).unwrap();
    let output = temp.path().join("output");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_import_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--archive",
            archive.to_str().unwrap(),
            "--expected-store-path",
            &store_path,
            "--expected-package-name",
            "example",
            "--expected-version",
            "1.0",
            "--expected-target-system",
            "x86_64-linux",
            "--expected-runtime-store-output",
            &store_path,
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!output.exists());
}

#[test]
fn rejects_store_archives_with_children_below_symlinks() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), "payload\n").unwrap();
    let key = "0123456789abcdef0123456789abcdef";
    let entry = format!("{key}-example-1.0");
    let store_path = format!("/pkgs/store/{entry}");
    let archive = temp.path().join("example.bpkgs-tree");
    let manifest = temp.path().join("example.manifest.json");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_object"))
        .args([
            "--input",
            source.to_str().unwrap(),
            "--store-path",
            &store_path,
            "--store-path-key",
            key,
            "--store-entry",
            &entry,
            "--package-name",
            "example",
            "--version",
            "1.0",
            "--target-system",
            "x86_64-linux",
            "--runtime-store-output",
            &store_path,
            "--archive",
            archive.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let mut payload = b"BUCKPKGS-STORE-ARCHIVE-V1\0".to_vec();
    append_archive_record(&mut payload, 1, b"", None);
    append_archive_record(
        &mut payload,
        3,
        b"escape",
        Some(outside.as_os_str().as_encoded_bytes()),
    );
    append_archive_record(&mut payload, 2, b"escape/payload", Some(b"owned\n"));
    payload.push(0);
    fs::write(&archive, &payload).unwrap();

    let hash = format!("sha256:{:x}", Sha256::digest(&payload));
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["archive"]["download_hash"] = Value::String(hash.clone());
    value["archive"]["payload_hash"] = Value::String(hash.clone());
    value["archive"]["download_size"] = Value::from(payload.len() as u64);
    value["archive"]["payload_size"] = Value::from(payload.len() as u64);
    value["canonical_tree_hash"] = Value::String(hash);
    fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let output = temp.path().join("output");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_import_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--archive",
            archive.to_str().unwrap(),
            "--expected-store-path",
            &store_path,
            "--expected-package-name",
            "example",
            "--expected-version",
            "1.0",
            "--expected-target-system",
            "x86_64-linux",
            "--expected-runtime-store-output",
            &store_path,
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!outside.join("payload").exists());
}

#[test]
fn rejects_store_archives_with_noncanonical_traversal_order() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), "payload\n").unwrap();
    let key = "0123456789abcdef0123456789abcdef";
    let entry = format!("{key}-example-1.0");
    let store_path = format!("/pkgs/store/{entry}");
    let archive = temp.path().join("example.bpkgs-tree");
    let manifest = temp.path().join("example.manifest.json");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_object"))
        .args([
            "--input",
            source.to_str().unwrap(),
            "--store-path",
            &store_path,
            "--store-path-key",
            key,
            "--store-entry",
            &entry,
            "--package-name",
            "example",
            "--version",
            "1.0",
            "--target-system",
            "x86_64-linux",
            "--runtime-store-output",
            &store_path,
            "--archive",
            archive.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let mut payload = b"BUCKPKGS-STORE-ARCHIVE-V1\0".to_vec();
    append_archive_record(&mut payload, 1, b"", None);
    append_archive_record(&mut payload, 2, b"z", Some(b"last\n"));
    append_archive_record(&mut payload, 2, b"a", Some(b"first\n"));
    payload.push(0);
    fs::write(&archive, &payload).unwrap();

    let hash = format!("sha256:{:x}", Sha256::digest(&payload));
    let mut value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    value["archive"]["download_hash"] = Value::String(hash.clone());
    value["archive"]["payload_hash"] = Value::String(hash.clone());
    value["archive"]["download_size"] = Value::from(payload.len() as u64);
    value["archive"]["payload_size"] = Value::from(payload.len() as u64);
    value["canonical_tree_hash"] = Value::String(hash);
    fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let output = temp.path().join("output");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_import_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--archive",
            archive.to_str().unwrap(),
            "--expected-store-path",
            &store_path,
            "--expected-package-name",
            "example",
            "--expected-version",
            "1.0",
            "--expected-target-system",
            "x86_64-linux",
            "--expected-runtime-store-output",
            &store_path,
            "--output",
            output.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!output.exists());
}

#[test]
fn exports_and_hydrates_complete_store_closures() {
    let temp = tempdir().unwrap();
    let dependency_source = temp.path().join("dependency-source");
    let root_source = temp.path().join("root-source");
    fs::create_dir_all(&dependency_source).unwrap();
    fs::create_dir_all(&root_source).unwrap();
    fs::write(dependency_source.join("dependency"), "dependency\n").unwrap();
    fs::write(root_source.join("root"), "root\n").unwrap();

    let dependency_key = "11111111111111111111111111111111";
    let dependency_entry = format!("{dependency_key}-dependency-1.0");
    let dependency_path = format!("/pkgs/store/{dependency_entry}");
    let dependency_archive = temp.path().join("dependency.bpkgs-tree");
    let dependency_manifest = temp.path().join("dependency.manifest.json");
    export_test_store_object(
        &dependency_source,
        dependency_key,
        &dependency_entry,
        "dependency",
        &[],
        &[&dependency_path],
        &dependency_archive,
        &dependency_manifest,
    );

    let root_key = "22222222222222222222222222222222";
    let root_entry = format!("{root_key}-root-1.0");
    let root_path = format!("/pkgs/store/{root_entry}");
    let root_archive = temp.path().join("root.bpkgs-tree");
    let root_manifest = temp.path().join("root.manifest.json");
    export_test_store_object(
        &root_source,
        root_key,
        &root_entry,
        "root",
        &[&dependency_path],
        &[&dependency_path, &root_path],
        &root_archive,
        &root_manifest,
    );

    let bundle = temp.path().join("bundle");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_closure"))
        .args([
            "--name",
            "test-closure",
            "--target-system",
            "x86_64-linux",
            "--root",
            &root_path,
            "--object-manifest",
            dependency_manifest.to_str().unwrap(),
            "--object-archive",
            dependency_archive.to_str().unwrap(),
            "--object-manifest",
            root_manifest.to_str().unwrap(),
            "--object-archive",
            root_archive.to_str().unwrap(),
            "--output",
            bundle.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let closure = fs::read_to_string(bundle.join("closure.json")).unwrap();
    assert!(closure.contains("\"format\": \"buckpkgs-store-closure-v1\""));
    assert!(closure.contains(&dependency_path));
    assert!(closure.contains(&root_path));

    let store_root = temp.path().join("hydrated-store");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_hydrate_store_closure"))
        .args([
            "--closure",
            bundle.join("closure.json").to_str().unwrap(),
            "--bundle",
            bundle.to_str().unwrap(),
            "--store-root",
            store_root.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(store_root.join(dependency_entry).join("dependency")).unwrap(),
        "dependency\n"
    );
    assert_eq!(
        fs::read_to_string(store_root.join(root_entry).join("root")).unwrap(),
        "root\n"
    );

    let incomplete = temp.path().join("incomplete");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_closure"))
        .args([
            "--name",
            "test-closure",
            "--target-system",
            "x86_64-linux",
            "--root",
            &root_path,
            "--object-manifest",
            root_manifest.to_str().unwrap(),
            "--object-archive",
            root_archive.to_str().unwrap(),
            "--output",
            incomplete.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!incomplete.exists());
}

#[test]
fn projects_verified_hydrated_store_objects_and_reports_missing_hydration() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("payload"), "payload\n").unwrap();
    let key = "33333333333333333333333333333333";
    let entry = format!("{key}-projected-1.0");
    let store_path = format!("/pkgs/store/{entry}");
    let archive = temp.path().join("projected.bpkgs-tree");
    let manifest = temp.path().join("projected.manifest.json");
    export_test_store_object(
        &source,
        key,
        &entry,
        "projected",
        &[],
        &[&store_path],
        &archive,
        &manifest,
    );

    let store_root = temp.path().join("store");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_hydrate_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--archive",
            archive.to_str().unwrap(),
            "--store-root",
            store_root.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let projected = temp.path().join("projected-output");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_project_hydrated_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--store-root",
            store_root.to_str().unwrap(),
            "--expected-store-path",
            &store_path,
            "--expected-package-name",
            "projected",
            "--expected-version",
            "1.0",
            "--expected-target-system",
            "x86_64-linux",
            "--expected-runtime-store-output",
            &store_path,
            "--output",
            projected.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(projected.join("payload")).unwrap(),
        "payload\n"
    );

    let missing_output = Command::new(env!("CARGO_BIN_EXE_pkgs_project_hydrated_store_object"))
        .args([
            "--manifest",
            manifest.to_str().unwrap(),
            "--store-root",
            temp.path().join("missing-store").to_str().unwrap(),
            "--expected-store-path",
            &store_path,
            "--expected-package-name",
            "projected",
            "--expected-version",
            "1.0",
            "--expected-target-system",
            "x86_64-linux",
            "--expected-runtime-store-output",
            &store_path,
            "--missing-hint",
            "run the hydration command",
            "--output",
            temp.path().join("missing-output").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!missing_output.status.success());
    let stderr = String::from_utf8(missing_output.stderr).unwrap();
    assert!(stderr.contains(&store_path));
    assert!(stderr.contains("run the hydration command"));
}

fn append_archive_record(payload: &mut Vec<u8>, kind: u8, path: &[u8], contents: Option<&[u8]>) {
    payload.push(kind);
    payload.extend_from_slice(&(path.len() as u32).to_be_bytes());
    payload.extend_from_slice(path);
    if let Some(contents) = contents {
        if kind == 2 {
            payload.push(0);
            payload.extend_from_slice(&(contents.len() as u64).to_be_bytes());
        } else {
            payload.extend_from_slice(&(contents.len() as u32).to_be_bytes());
        }
        payload.extend_from_slice(contents);
    }
}

fn export_test_store_object(
    source: &Path,
    key: &str,
    entry: &str,
    package_name: &str,
    references: &[&str],
    runtime_store_outputs: &[&str],
    archive: &Path,
    manifest: &Path,
) {
    let store_path = format!("/pkgs/store/{entry}");
    let mut command = Command::new(env!("CARGO_BIN_EXE_pkgs_export_store_object"));
    command.args([
        "--input",
        source.to_str().unwrap(),
        "--store-path",
        &store_path,
        "--store-path-key",
        key,
        "--store-entry",
        entry,
        "--package-name",
        package_name,
        "--version",
        "1.0",
        "--target-system",
        "x86_64-linux",
        "--archive",
        archive.to_str().unwrap(),
        "--manifest",
        manifest.to_str().unwrap(),
    ]);
    for reference in references {
        command.args(["--reference", reference]);
    }
    for runtime_store_output in runtime_store_outputs {
        command.args(["--runtime-store-output", runtime_store_output]);
    }
    assert!(command.status().unwrap().success());
}

#[test]
fn rejects_forbidden_references() {
    let temp = tempdir().unwrap();
    let tree = temp.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    fs::write(
        tree.join("payload"),
        b"uses /pkgs/store/foreign-seed/bin/bash\n",
    )
    .unwrap();

    let stamp = temp.path().join("stamp");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_verify_no_refs"))
        .args([
            "--input",
            tree.to_str().unwrap(),
            "--forbidden",
            "/pkgs/store/foreign-seed",
            "--stamp",
            stamp.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!stamp.exists());
}

#[test]
fn verifies_replayed_trees_byte_for_byte() {
    let temp = tempdir().unwrap();
    let expected = temp.path().join("expected");
    let actual = temp.path().join("actual");
    fs::create_dir_all(expected.join("share")).unwrap();
    fs::create_dir_all(actual.join("share")).unwrap();
    fs::write(expected.join("share/payload"), b"reproducible\n").unwrap();
    fs::write(actual.join("share/payload"), b"reproducible\n").unwrap();
    for path in [
        expected.clone(),
        expected.join("share"),
        expected.join("share/payload"),
        actual.clone(),
        actual.join("share"),
        actual.join("share/payload"),
    ] {
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1)))
            .unwrap();
    }

    let ok_stamp = temp.path().join("ok-stamp");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_verify_reproducible_tree"))
        .args([
            "--expected",
            expected.to_str().unwrap(),
            "--actual",
            actual.to_str().unwrap(),
            "--stamp",
            ok_stamp.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(ok_stamp.exists());

    fs::write(actual.join("share/payload"), b"drifted\n").unwrap();
    let bad_stamp = temp.path().join("bad-stamp");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_verify_reproducible_tree"))
        .args([
            "--expected",
            expected.to_str().unwrap(),
            "--actual",
            actual.to_str().unwrap(),
            "--stamp",
            bad_stamp.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!bad_stamp.exists());
}

#[test]
fn verifies_archive_metadata_headers() {
    let temp = tempdir().unwrap();
    let tree = temp.path().join("tree");
    fs::create_dir_all(tree.join("lib")).unwrap();
    fs::create_dir_all(tree.join("share")).unwrap();

    let deterministic_archive = format!(
        "!<arch>\n{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\nobj\n",
        "payload.o/", "0", "0", "0", "100644", 4,
    );
    fs::write(tree.join("lib/libexample.a"), deterministic_archive).unwrap();
    fs::write(
        tree.join("share/manual.gz"),
        [0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();

    let ok_stamp = temp.path().join("ok-archive-metadata");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_verify_archive_metadata"))
        .args([
            "--input",
            tree.to_str().unwrap(),
            "--stamp",
            ok_stamp.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(ok_stamp.exists());

    fs::write(
        tree.join("share/manual.gz"),
        [0x1f, 0x8b, 0x08, 0, 1, 0, 0, 0, 0, 0],
    )
    .unwrap();
    let bad_stamp = temp.path().join("bad-archive-metadata");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_verify_archive_metadata"))
        .args([
            "--input",
            tree.to_str().unwrap(),
            "--stamp",
            bad_stamp.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!bad_stamp.exists());
}

#[test]
fn supports_out_of_source_configure_builds() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
test "$PWD" != "$(cd "$(dirname "$0")" && pwd)"
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
cat > Makefile <<EOF
all:
	printf built > artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
            "--out-of-source",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(output.join("bin/artifact")).unwrap(),
        "built"
    );
}

#[test]
fn preserves_the_logical_install_prefix_in_installed_files() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
cat > Makefile <<EOF
all:
	:
install:
	mkdir -p "\$(DESTDIR)$prefix/lib"
	printf '%s\n' "$prefix/lib/libexample.so.1" > "\$(DESTDIR)$prefix/lib/libexample.so"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(output.join("lib/libexample.so")).unwrap(),
        "/pkgs/store/example-package/lib/libexample.so.1\n"
    );
}

#[test]
fn uses_a_stable_configure_work_dir_under_buck_scratch_path() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
cat > Makefile <<EOF
all:
	printf '%s\n' "$PWD" > artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let scratch = temp.path().join("buck-scratch");
    let first_output = temp.path().join("first-out");
    let second_output = temp.path().join("second-out");

    for output in [&first_output, &second_output] {
        let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
            .env("BUCK_SCRATCH_PATH", &scratch)
            .args([
                "--source",
                source.to_str().unwrap(),
                "--output",
                output.to_str().unwrap(),
                "--install-prefix",
                "/pkgs/store/example-package",
                "--path-entry",
                "/usr/bin",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }

    assert_eq!(
        fs::read_to_string(first_output.join("bin/artifact")).unwrap(),
        fs::read_to_string(second_output.join("bin/artifact")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(first_output.join("bin/artifact")).unwrap(),
        format!("{}/pkgs-configure-make-install/source\n", scratch.display())
    );
}

#[test]
fn configures_with_a_reproducible_environment() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
cat > Makefile <<EOF
all:
	printf '%s:%s:%s:%s:%s:%s\n' "$LC_ALL" "$LANG" "$TZ" "$SOURCE_DATE_EPOCH" "$PYTHONHASHSEED" "$PERL_HASH_SEED" > artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .env("LC_ALL", "fr_FR.UTF-8")
        .env("LANG", "fr_FR.UTF-8")
        .env("TZ", "America/Los_Angeles")
        .env("SOURCE_DATE_EPOCH", "999999999")
        .env("PYTHONHASHSEED", "999")
        .env("PERL_HASH_SEED", "999")
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(output.join("bin/artifact")).unwrap(),
        "C:C:UTC:1:0:0\n"
    );
}

#[test]
fn configures_with_an_explicit_make_job_count() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
cat > Makefile <<EOF
all:
	printf '%s\n' "\$(MAKEFLAGS)" > artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
            "--make-jobs",
            "7",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let makeflags = fs::read_to_string(output.join("bin/artifact")).unwrap();
    assert!(
        makeflags.contains("-j7"),
        "MAKEFLAGS did not carry the declared job count: {makeflags:?}",
    );
}

#[test]
fn does_not_inherit_ambient_host_environment() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
printf '%s\n' "${HOST_POLLUTION-unset}" > artifact
cat > Makefile <<EOF
all:
	:
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .env("HOST_POLLUTION", "should-not-leak")
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(output.join("bin/artifact")).unwrap(),
        "unset\n"
    );
}

#[test]
fn remaps_absolute_build_paths_in_compiled_outputs() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("artifact.c"),
        r#"#include <stdio.h>
int main(void) {
    puts(__FILE__);
    return 0;
}
"#,
    )
    .unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
source_path=$PWD
cat > Makefile <<EOF
all:
	cc \$(CFLAGS) "$source_path/artifact.c" -o artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let scratch = temp.path().join("buck-scratch");
    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .env("BUCK_SCRATCH_PATH", &scratch)
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let binary = fs::read(output.join("bin/artifact")).unwrap();
    let leaked = format!(
        "{}/pkgs-configure-make-install/source/artifact.c",
        scratch.display()
    );
    assert!(!binary
        .windows(leaked.len())
        .any(|window| window == leaked.as_bytes()));
    assert!(binary
        .windows("./source/artifact.c".len())
        .any(|window| window == b"./source/artifact.c"));
}

#[test]
fn does_not_install_prefix_map_flags_as_package_metadata() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
cat > Makefile <<EOF
all:
	printf '%s\n' "\$\$CFLAGS" > artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/share"
	cp artifact "\$(DESTDIR)$prefix/share/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();

    let scratch = temp.path().join("buck-scratch");
    let output = temp.path().join("out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .env("BUCK_SCRATCH_PATH", &scratch)
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(output.join("share/artifact")).unwrap(),
        "\n"
    );
}

#[test]
fn reuses_an_explicit_configure_work_dir_only_when_requested() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("configure"),
        r#"#!/bin/sh
set -eu
prefix=
for arg in "$@"; do
  case "$arg" in
    --prefix=*) prefix=${arg#--prefix=} ;;
  esac
done
printf x >> configure-runs
cat > Makefile <<EOF
all:
	printf built > artifact
install:
	mkdir -p "\$(DESTDIR)$prefix/bin"
	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
EOF
"#,
    )
    .unwrap();
    let configure = source.join("configure");
    let mut permissions = fs::metadata(&configure).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&configure, permissions).unwrap();
    let patch = temp.path().join("configure.patch");
    fs::write(
        &patch,
        r#"--- a/configure
+++ b/configure
@@ -10,7 +10,7 @@
 printf x >> configure-runs
 cat > Makefile <<EOF
 all:
-	printf built > artifact
+	printf patched > artifact
 install:
 	mkdir -p "\$(DESTDIR)$prefix/bin"
 	cp artifact "\$(DESTDIR)$prefix/bin/artifact"
"#,
    )
    .unwrap();

    let work_dir = temp.path().join("work");
    let first_output = temp.path().join("first-out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            first_output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
            "--out-of-source",
            "--work-dir",
            work_dir.to_str().unwrap(),
            "--patch",
            patch.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(work_dir.join("build/artifact").exists());

    let second_output = temp.path().join("second-out");
    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            second_output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
            "--out-of-source",
            "--work-dir",
            work_dir.to_str().unwrap(),
            "--patch",
            patch.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(!status.success());

    let status = Command::new(env!("CARGO_BIN_EXE_pkgs_configure_make_install"))
        .args([
            "--source",
            source.to_str().unwrap(),
            "--output",
            second_output.to_str().unwrap(),
            "--install-prefix",
            "/pkgs/store/example-package",
            "--path-entry",
            "/usr/bin",
            "--out-of-source",
            "--work-dir",
            work_dir.to_str().unwrap(),
            "--reuse-work-dir",
            "--patch",
            patch.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        fs::read_to_string(work_dir.join("build/configure-runs")).unwrap(),
        "xx"
    );
    assert_eq!(
        fs::read_to_string(second_output.join("bin/artifact")).unwrap(),
        "patched"
    );
}
