use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const PT_INTERP: u32 = 3;

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-elf-interpreters")]
pub(crate) struct Args {
    #[arg(long)]
    input: Vec<PathBuf>,

    #[arg(long)]
    expected_interpreter: String,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("ELF file {path} uses interpreter {actual:?}, expected {expected:?}")]
    UnexpectedInterpreter {
        path: PathBuf,
        actual: String,
        expected: String,
    },
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    for input in &args.input {
        verify_tree(input, &args.expected_interpreter)?;
    }

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

fn verify_tree(path: &Path, expected_interpreter: &str) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            verify_tree(&entry.path(), expected_interpreter)?;
        }
        return Ok(());
    }

    let bytes = fs::read(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    if let Some(actual) = interpreter(&bytes) {
        if actual != expected_interpreter {
            return Err(Error::UnexpectedInterpreter {
                path: path.to_path_buf(),
                actual,
                expected: expected_interpreter.to_owned(),
            });
        }
    }
    Ok(())
}

fn interpreter(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 16 || &bytes[..4] != ELF_MAGIC || bytes[5] != ELFDATA2LSB {
        return None;
    }

    let (phoff, phentsize, phnum, offset_offset, filesz_offset) = match bytes[4] {
        ELFCLASS32 => (
            read_u32(bytes, 28)? as usize,
            read_u16(bytes, 42)? as usize,
            read_u16(bytes, 44)? as usize,
            4,
            16,
        ),
        ELFCLASS64 => (
            read_u64(bytes, 32)? as usize,
            read_u16(bytes, 54)? as usize,
            read_u16(bytes, 56)? as usize,
            8,
            32,
        ),
        _ => return None,
    };

    for index in 0..phnum {
        let entry = phoff.checked_add(index.checked_mul(phentsize)?)?;
        if read_u32(bytes, entry)? != PT_INTERP {
            continue;
        }

        let offset = read_word(bytes, entry + offset_offset, bytes[4])? as usize;
        let filesz = read_word(bytes, entry + filesz_offset, bytes[4])? as usize;
        let end = offset.checked_add(filesz)?;
        let raw = bytes.get(offset..end)?;
        let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
        return std::str::from_utf8(raw).ok().map(ToOwned::to_owned);
    }

    None
}

fn read_word(bytes: &[u8], offset: usize, class: u8) -> Option<u64> {
    match class {
        ELFCLASS32 => read_u32(bytes, offset).map(u64::from),
        ELFCLASS64 => read_u64(bytes, offset),
        _ => None,
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(raw))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let raw = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_64_bit_interpreters() {
        let bytes = elf64_with_interpreter("/pkgs/store/glibc/lib/ld-linux.so.2");
        assert_eq!(
            interpreter(&bytes).as_deref(),
            Some("/pkgs/store/glibc/lib/ld-linux.so.2")
        );
    }

    #[test]
    fn ignores_non_elf_files() {
        assert_eq!(interpreter(b"plain text"), None);
    }

    fn elf64_with_interpreter(value: &str) -> Vec<u8> {
        let interp = format!("{value}\0");
        let mut bytes = vec![0; 64 + 56 + interp.len()];
        bytes[..4].copy_from_slice(ELF_MAGIC);
        bytes[4] = ELFCLASS64;
        bytes[5] = ELFDATA2LSB;
        bytes[32..40].copy_from_slice(&(64_u64).to_le_bytes());
        bytes[54..56].copy_from_slice(&(56_u16).to_le_bytes());
        bytes[56..58].copy_from_slice(&(1_u16).to_le_bytes());

        let ph = 64;
        bytes[ph..ph + 4].copy_from_slice(&PT_INTERP.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&(120_u64).to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&(interp.len() as u64).to_le_bytes());
        bytes[120..120 + interp.len()].copy_from_slice(interp.as_bytes());
        bytes
    }
}
