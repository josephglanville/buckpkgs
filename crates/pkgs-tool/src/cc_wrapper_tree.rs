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
    libc_runtime: PathBuf,

    #[arg(long)]
    libc_dev: PathBuf,

    #[arg(long)]
    gcc_dev: PathBuf,

    #[arg(long)]
    cxx_include_dir: Vec<PathBuf>,

    #[arg(long)]
    libgcc_runtime: PathBuf,

    #[arg(long)]
    libstdcxx_runtime: PathBuf,

    #[arg(long)]
    suppress_cxx_runtime_rpaths: bool,

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

    let sysroot = format!("--sysroot={}", args.libc_dev.display());
    let bintools = format!("-B{}/bin/", args.bintools.display());
    let crt = format!("-B{}/lib/", args.libc_dev.display());
    let headers_flag = "-idirafter".to_owned();
    let headers_dir = format!("{}/include", args.headers.display());
    let dynamic_linker = format!("-Wl,-dynamic-linker,{}", args.dynamic_linker.display());
    let libc_runtime_path = format!("-Wl,-rpath,{}/lib", args.libc_runtime.display());
    let gcc_dev_search_path = format!("-L{}/lib", args.gcc_dev.display());
    let gcc_dev_search_path64 = format!("-L{}/lib64", args.gcc_dev.display());
    let libgcc_runtime_path = format!("-Wl,-rpath,{}/lib", args.libgcc_runtime.display());
    let libgcc_runtime_path64 = format!("-Wl,-rpath,{}/lib64", args.libgcc_runtime.display());
    let libstdcxx_runtime_path = format!("-Wl,-rpath,{}/lib", args.libstdcxx_runtime.display());
    let libstdcxx_runtime_path64 = format!("-Wl,-rpath,{}/lib64", args.libstdcxx_runtime.display());
    let split_gcc_interface = !args.cc.starts_with(&args.gcc_dev);
    let mut c_flags = vec![
        &sysroot,
        &bintools,
        &crt,
        &headers_flag,
        &headers_dir,
        &dynamic_linker,
        &libc_runtime_path,
    ];
    if split_gcc_interface {
        c_flags.extend([&gcc_dev_search_path, &gcc_dev_search_path64]);
    }
    let mut cxx_flags = vec![
        &sysroot,
        &bintools,
        &crt,
        &headers_flag,
        &headers_dir,
        &dynamic_linker,
        &libc_runtime_path,
        &gcc_dev_search_path,
        &gcc_dev_search_path64,
    ];
    if !args.suppress_cxx_runtime_rpaths {
        cxx_flags.extend([
            &libgcc_runtime_path,
            &libgcc_runtime_path64,
            &libstdcxx_runtime_path,
            &libstdcxx_runtime_path64,
        ]);
    }
    let cxx_include_flags: Vec<String> = args
        .cxx_include_dir
        .iter()
        .flat_map(|path| ["-isystem".to_owned(), path.display().to_string()])
        .collect();
    cxx_flags.extend(cxx_include_flags.iter());

    for name in ["cc", "gcc"] {
        write_wrapper(
            &bin_dir.join(name),
            &args.shell,
            &args.cc,
            c_flags.iter().copied(),
        )?;
    }

    for name in ["c++", "g++"] {
        write_wrapper(
            &bin_dir.join(name),
            &args.shell,
            &args.cxx,
            cxx_flags.iter().copied(),
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
            libc_runtime: PathBuf::from("/pkgs/store/glibc-lib"),
            libc_dev: PathBuf::from("/pkgs/store/glibc-dev"),
            gcc_dev: PathBuf::from("/pkgs/store/gcc-dev"),
            cxx_include_dir: vec![PathBuf::from("/pkgs/store/gcc-dev/include/c++/15.2.0")],
            libgcc_runtime: PathBuf::from("/pkgs/store/libgcc-lib"),
            libstdcxx_runtime: PathBuf::from("/pkgs/store/libstdcxx-lib"),
            suppress_cxx_runtime_rpaths: false,
            bintools: PathBuf::from("/pkgs/store/binutils-wrapper"),
            headers: PathBuf::from("/pkgs/store/linux-headers"),
            dynamic_linker: PathBuf::from("/pkgs/store/glibc-lib/lib/ld-linux-x86-64.so.2"),
        };

        run(&args).unwrap();

        let gcc = fs::read_to_string(output.join("bin/gcc")).unwrap();
        assert!(gcc.contains("--sysroot=/pkgs/store/glibc-dev"));
        assert!(gcc.contains("-B/pkgs/store/binutils-wrapper/bin/"));
        assert!(gcc.contains("'-idirafter' '/pkgs/store/linux-headers/include'"));
        assert!(gcc.contains("-Wl,-dynamic-linker,/pkgs/store/glibc-lib/lib/ld-linux-x86-64.so.2"));
        assert!(gcc.contains("-Wl,-rpath,/pkgs/store/glibc-lib/lib"));
        assert!(gcc.contains("-L/pkgs/store/gcc-dev/lib"));
        assert!(!gcc.contains("-Wl,-rpath,/pkgs/store/libgcc-lib/lib"));
        assert!(!gcc.contains("-Wl,-rpath,/pkgs/store/libstdcxx-lib/lib"));

        let gxx = fs::read_to_string(output.join("bin/g++")).unwrap();
        assert!(gxx.contains("-L/pkgs/store/gcc-dev/lib"));
        assert!(!gxx.contains("-L/pkgs/store/libgcc-lib/lib"));
        assert!(gxx.contains("-Wl,-rpath,/pkgs/store/libgcc-lib/lib"));
        assert!(!gxx.contains("-L/pkgs/store/libstdcxx-lib/lib"));
        assert!(gxx.contains("-Wl,-rpath,/pkgs/store/libstdcxx-lib/lib"));
        assert!(gxx.contains("'-isystem' '/pkgs/store/gcc-dev/include/c++/15.2.0'"));

        let cpp = fs::read_to_string(output.join("bin/cpp")).unwrap();
        assert!(cpp.contains("--sysroot=/pkgs/store/glibc-dev"));
        assert!(!cpp.contains("-Wl,-dynamic-linker"));

        let mut args = args;
        args.output = temp.path().join("build-only");
        args.suppress_cxx_runtime_rpaths = true;
        run(&args).unwrap();

        let gxx = fs::read_to_string(args.output.join("bin/g++")).unwrap();
        assert!(gxx.contains("-L/pkgs/store/gcc-dev/lib"));
        assert!(!gxx.contains("-Wl,-rpath,/pkgs/store/libgcc-lib/lib"));
        assert!(!gxx.contains("-Wl,-rpath,/pkgs/store/libstdcxx-lib/lib"));

        let gcc = fs::read_to_string(args.output.join("bin/gcc")).unwrap();
        assert!(gcc.contains("-L/pkgs/store/gcc-dev/lib"));
        assert!(!gcc.contains("-Wl,-rpath,/pkgs/store/libgcc-lib/lib"));

        args.output = temp.path().join("monolithic");
        args.gcc_dev = PathBuf::from("/pkgs/store/gcc");
        args.suppress_cxx_runtime_rpaths = false;
        run(&args).unwrap();

        let gcc = fs::read_to_string(args.output.join("bin/gcc")).unwrap();
        assert!(!gcc.contains("-L/pkgs/store/gcc-dev/lib"));
        assert!(!gcc.contains("-Wl,-rpath,/pkgs/store/libgcc-lib/lib"));
    }
}
