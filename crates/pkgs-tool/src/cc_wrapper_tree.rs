use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-cc-wrapper-tree")]
pub(crate) struct Args {
    #[arg(long)]
    output: PathBuf,

    #[arg(long)]
    shell: PathBuf,

    #[arg(long)]
    cc: PathBuf,

    #[arg(long)]
    cxx: PathBuf,

    #[arg(long)]
    cpp: PathBuf,

    #[arg(long)]
    compiler_root: PathBuf,

    #[arg(long)]
    libc: PathBuf,

    #[arg(long)]
    bintools: PathBuf,

    #[arg(long)]
    headers: PathBuf,

    #[arg(long)]
    dynamic_linker: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("failed to inspect {path}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    let bin_dir = args.output.join("bin");
    fs::create_dir_all(&bin_dir).map_err(|source| common::Error::CreateDir {
        path: bin_dir.clone(),
        source,
    })?;

    let sysroot = format!("--sysroot={}", args.libc.display());
    let bintools = format!("-B{}/bin/", args.bintools.display());
    let crt = format!("-B{}/lib/", args.libc.display());
    let headers_flag = "-idirafter".to_owned();
    let headers_dir = format!("{}/include", args.headers.display());
    let dynamic_linker = format!("-Wl,-dynamic-linker,{}", args.dynamic_linker.display());
    let libc_runtime_path = format!("-Wl,-rpath,{}/lib", args.libc.display());
    let compiler_runtime_lib_path = format!("-Wl,-rpath,{}/lib", args.compiler_root.display());
    let compiler_runtime_lib64_path = format!("-Wl,-rpath,{}/lib64", args.compiler_root.display());

    for (name, target) in [
        ("cc", &args.cc),
        ("gcc", &args.cc),
        ("c++", &args.cxx),
        ("g++", &args.cxx),
    ] {
        write_wrapper(
            &bin_dir.join(name),
            &args.shell,
            target,
            [
                &sysroot,
                &bintools,
                &crt,
                &headers_flag,
                &headers_dir,
                &dynamic_linker,
                &libc_runtime_path,
                &compiler_runtime_lib_path,
                &compiler_runtime_lib64_path,
            ],
        )?;
    }

    write_wrapper(
        &bin_dir.join("cpp"),
        &args.shell,
        &args.cpp,
        [&sysroot, &bintools, &crt, &headers_flag, &headers_dir],
    )?;

    common::normalize_tree_mtimes(&args.output)?;
    common::make_tree_read_only(&args.output)?;
    Ok(())
}

fn write_wrapper<'a>(
    path: &Path,
    shell: &Path,
    target: &Path,
    args: impl IntoIterator<Item = &'a String>,
) -> Result<(), Error> {
    let mut script = String::new();
    script.push_str("#!");
    script.push_str(&shell.display().to_string());
    script.push('\n');
    script.push_str("exec ");
    script.push_str(&shell_quote(&target.display().to_string()));
    for arg in args {
        script.push(' ');
        script.push_str(&shell_quote(arg));
    }
    script.push_str(" \"$@\"\n");

    fs::write(path, script).map_err(|source| common::Error::WriteFile {
        path: path.to_path_buf(),
        source,
    })?;

    let metadata = fs::metadata(path).map_err(|source| Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|source| common::Error::SetPermissions {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_words() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("has'quote"), "'has'\"'\"'quote'");
    }

    #[test]
    fn writes_compiler_and_preprocessor_wrappers() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("out");
        let args = Args {
            output: output.clone(),
            shell: PathBuf::from("/pkgs/store/bash/bin/bash"),
            cc: PathBuf::from("/pkgs/store/gcc/bin/gcc"),
            cxx: PathBuf::from("/pkgs/store/gcc/bin/g++"),
            cpp: PathBuf::from("/pkgs/store/gcc/bin/cpp"),
            compiler_root: PathBuf::from("/pkgs/store/gcc"),
            libc: PathBuf::from("/pkgs/store/glibc"),
            bintools: PathBuf::from("/pkgs/store/binutils-wrapper"),
            headers: PathBuf::from("/pkgs/store/linux-headers"),
            dynamic_linker: PathBuf::from("/pkgs/store/glibc/lib/ld-linux-x86-64.so.2"),
        };

        run(&args).unwrap();

        let gcc = fs::read_to_string(output.join("bin/gcc")).unwrap();
        assert!(gcc.contains("--sysroot=/pkgs/store/glibc"));
        assert!(gcc.contains("-B/pkgs/store/binutils-wrapper/bin/"));
        assert!(gcc.contains("'-idirafter' '/pkgs/store/linux-headers/include'"));
        assert!(gcc.contains("-Wl,-dynamic-linker,/pkgs/store/glibc/lib/ld-linux-x86-64.so.2"));
        assert!(gcc.contains("-Wl,-rpath,/pkgs/store/glibc/lib"));
        assert!(gcc.contains("-Wl,-rpath,/pkgs/store/gcc/lib"));
        assert!(gcc.contains("-Wl,-rpath,/pkgs/store/gcc/lib64"));

        let cpp = fs::read_to_string(output.join("bin/cpp")).unwrap();
        assert!(cpp.contains("--sysroot=/pkgs/store/glibc"));
        assert!(!cpp.contains("-Wl,-dynamic-linker"));
    }
}
