use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

#[derive(Debug, Parser)]
#[command(name = "pkgs-bintools-wrapper-tree")]
pub(crate) struct Args {
    #[arg(long)]
    output: PathBuf,

    #[arg(long)]
    shell: PathBuf,

    #[arg(long)]
    binutils: PathBuf,
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

    for command in [
        "addr2line",
        "ar",
        "as",
        "c++filt",
        "elfedit",
        "nm",
        "objcopy",
        "objdump",
        "ranlib",
        "readelf",
        "size",
        "strings",
        "strip",
    ] {
        let target = args.binutils.join("bin").join(command);
        let link = bin_dir.join(command);
        symlink(&target, &link).map_err(|source| common::Error::CreateSymlink {
            from: target,
            to: link,
            source,
        })?;
    }

    write_ld_wrapper(
        &bin_dir.join("ld"),
        &args.shell,
        &args.binutils.join("bin/ld"),
    )?;

    common::normalize_tree_mtimes(&args.output)?;
    common::make_tree_read_only(&args.output)?;
    Ok(())
}

fn write_ld_wrapper(path: &Path, shell: &Path, target: &Path) -> Result<(), Error> {
    let script = format!(
        "#!{}\n\
         set -e\n\
         args=()\n\
         while (($#)); do\n\
           case \"$1\" in\n\
             --sysroot)\n\
               shift\n\
               (($#)) && shift\n\
               ;;\n\
             --sysroot=*)\n\
               shift\n\
               ;;\n\
             *)\n\
               args+=(\"$1\")\n\
               shift\n\
               ;;\n\
           esac\n\
         done\n\
         exec {} \"${{args[@]}}\"\n",
        shell.display(),
        shell_quote(&target.display().to_string()),
    );

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
    fn writes_ld_wrapper_and_passthrough_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("out");
        let args = Args {
            output: output.clone(),
            shell: PathBuf::from("/pkgs/store/bash/bin/bash"),
            binutils: PathBuf::from("/pkgs/store/binutils"),
        };

        run(&args).unwrap();

        let ld = fs::read_to_string(output.join("bin/ld")).unwrap();
        assert!(ld.contains("--sysroot=*"));
        assert!(ld.contains("exec '/pkgs/store/binutils/bin/ld'"));

        assert_eq!(
            fs::read_link(output.join("bin/ar")).unwrap(),
            PathBuf::from("/pkgs/store/binutils/bin/ar"),
        );
    }
}
