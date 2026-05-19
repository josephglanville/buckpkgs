use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use tempfile::tempdir;

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
