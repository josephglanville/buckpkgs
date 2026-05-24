use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELFCLASS32: u8 = 1;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-elf-interpreters")]
pub(crate) struct Args {
    #[arg(long)]
    input: Vec<PathBuf>,

    #[arg(long)]
    expected_interpreter: String,

    #[arg(long = "allowed-runtime-prefix")]
    allowed_runtime_prefixes: Vec<String>,

    #[arg(long = "required-needed")]
    required_needed: Vec<String>,

    #[arg(long = "forbidden-needed")]
    forbidden_needed: Vec<String>,

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

    #[error(
        "ELF file {path} retains runtime search path {actual:?} outside declared runtime providers"
    )]
    UnexpectedRuntimeSearchPath { path: PathBuf, actual: String },

    #[error("ELF executable {path} does not require declared shared library {required:?}")]
    MissingNeededLibrary { path: PathBuf, required: String },

    #[error("ELF executable {path} unexpectedly requires shared library {forbidden:?}")]
    ForbiddenNeededLibrary { path: PathBuf, forbidden: String },
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    for input in &args.input {
        verify_tree(
            input,
            &args.expected_interpreter,
            &args.allowed_runtime_prefixes,
            &args.required_needed,
            &args.forbidden_needed,
        )?;
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

fn verify_tree(
    path: &Path,
    expected_interpreter: &str,
    allowed_runtime_prefixes: &[String],
    required_needed: &[String],
    forbidden_needed: &[String],
) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            verify_tree(
                &entry.path(),
                expected_interpreter,
                allowed_runtime_prefixes,
                required_needed,
                forbidden_needed,
            )?;
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
        let needed = needed_libraries(&bytes);
        for required in required_needed {
            if !needed.contains(required) {
                return Err(Error::MissingNeededLibrary {
                    path: path.to_path_buf(),
                    required: required.clone(),
                });
            }
        }
        for forbidden in forbidden_needed {
            if needed.contains(forbidden) {
                return Err(Error::ForbiddenNeededLibrary {
                    path: path.to_path_buf(),
                    forbidden: forbidden.clone(),
                });
            }
        }
    }
    if !allowed_runtime_prefixes.is_empty() {
        for actual in runtime_search_paths(&bytes)
            .iter()
            .flat_map(|paths| paths.split(':'))
            .filter(|path| path.starts_with('/'))
        {
            if !allowed_runtime_prefixes
                .iter()
                .any(|allowed| is_under_runtime_prefix(actual, allowed))
            {
                return Err(Error::UnexpectedRuntimeSearchPath {
                    path: path.to_path_buf(),
                    actual: actual.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn interpreter(bytes: &[u8]) -> Option<String> {
    for header in program_headers(bytes)? {
        if header.kind != PT_INTERP {
            continue;
        }
        let end = header.offset.checked_add(header.filesz)?;
        let raw = bytes.get(header.offset..end)?;
        let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
        return std::str::from_utf8(raw).ok().map(ToOwned::to_owned);
    }

    None
}

#[derive(Clone, Copy)]
struct ProgramHeader {
    kind: u32,
    offset: usize,
    vaddr: u64,
    filesz: usize,
}

fn program_headers(bytes: &[u8]) -> Option<Vec<ProgramHeader>> {
    if bytes.len() < 16 || &bytes[..4] != ELF_MAGIC || bytes[5] != ELFDATA2LSB {
        return None;
    }
    let (phoff, phentsize, phnum, offset_offset, vaddr_offset, filesz_offset) = match bytes[4] {
        ELFCLASS32 => (
            read_u32(bytes, 28)? as usize,
            read_u16(bytes, 42)? as usize,
            read_u16(bytes, 44)? as usize,
            4,
            8,
            16,
        ),
        ELFCLASS64 => (
            read_u64(bytes, 32)? as usize,
            read_u16(bytes, 54)? as usize,
            read_u16(bytes, 56)? as usize,
            8,
            16,
            32,
        ),
        _ => return None,
    };

    let mut headers = Vec::new();
    for index in 0..phnum {
        let entry = phoff.checked_add(index.checked_mul(phentsize)?)?;
        headers.push(ProgramHeader {
            kind: read_u32(bytes, entry)?,
            offset: read_word(bytes, entry + offset_offset, bytes[4])? as usize,
            vaddr: read_word(bytes, entry + vaddr_offset, bytes[4])?,
            filesz: read_word(bytes, entry + filesz_offset, bytes[4])? as usize,
        });
    }
    Some(headers)
}

fn runtime_search_paths(bytes: &[u8]) -> Vec<String> {
    dynamic_strings(bytes, &[DT_RPATH, DT_RUNPATH])
}

fn needed_libraries(bytes: &[u8]) -> Vec<String> {
    dynamic_strings(bytes, &[DT_NEEDED])
}

fn dynamic_strings(bytes: &[u8], requested_tags: &[u64]) -> Vec<String> {
    let Some(headers) = program_headers(bytes) else {
        return vec![];
    };
    let Some(dynamic) = headers.iter().find(|header| header.kind == PT_DYNAMIC) else {
        return vec![];
    };
    let entry_size = match bytes[4] {
        ELFCLASS32 => 8,
        ELFCLASS64 => 16,
        _ => return vec![],
    };
    let mut string_table_vaddr = None;
    let mut offsets = Vec::new();
    let Some(end) = dynamic.offset.checked_add(dynamic.filesz) else {
        return vec![];
    };
    for entry in (dynamic.offset..end).step_by(entry_size) {
        let Some(tag) = read_word(bytes, entry, bytes[4]) else {
            return vec![];
        };
        let Some(value) = read_word(bytes, entry + entry_size / 2, bytes[4]) else {
            return vec![];
        };
        match tag {
            DT_NULL => break,
            DT_STRTAB => string_table_vaddr = Some(value),
            tag if requested_tags.contains(&tag) => offsets.push(value as usize),
            _ => {}
        }
    }
    let Some(string_table_vaddr) = string_table_vaddr else {
        return vec![];
    };
    let Some(string_table_offset) = headers
        .iter()
        .find(|header| {
            header.kind == PT_LOAD
                && string_table_vaddr >= header.vaddr
                && string_table_vaddr < header.vaddr + header.filesz as u64
        })
        .map(|header| header.offset + (string_table_vaddr - header.vaddr) as usize)
    else {
        return vec![];
    };
    offsets
        .into_iter()
        .filter_map(|offset| read_c_string(bytes, string_table_offset.checked_add(offset)?))
        .collect()
}

fn read_c_string(bytes: &[u8], start: usize) -> Option<String> {
    let bytes = bytes.get(start..)?;
    let end = bytes.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&bytes[..end])
        .ok()
        .map(ToOwned::to_owned)
}

fn is_under_runtime_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
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

    #[test]
    fn extracts_runtime_search_paths() {
        assert_eq!(
            runtime_search_paths(&elf64_with_dynamic_strings(&[(
                DT_RUNPATH,
                "/pkgs/store/glibc/lib:/pkgs/store/libcap/lib",
            )])),
            vec!["/pkgs/store/glibc/lib:/pkgs/store/libcap/lib"],
        );
    }

    #[test]
    fn extracts_needed_libraries() {
        assert_eq!(
            needed_libraries(&elf64_with_dynamic_strings(&[
                (DT_NEEDED, "libstdc++.so.6"),
                (DT_NEEDED, "libc.so.6"),
            ])),
            vec!["libstdc++.so.6", "libc.so.6"],
        );
    }

    #[test]
    fn rejects_runtime_search_paths_outside_allowed_providers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("artifact");
        fs::write(
            &path,
            elf64_with_dynamic_strings(&[(DT_RUNPATH, "/pkgs/store/foreign/lib")]),
        )
        .unwrap();
        let result = verify_tree(
            &path,
            "/pkgs/store/glibc/lib/ld-linux.so.2",
            &["/pkgs/store/glibc".to_owned()],
            &[],
            &[],
        );
        assert!(matches!(
            result,
            Err(Error::UnexpectedRuntimeSearchPath { .. })
        ));
    }

    #[test]
    fn checks_required_and_forbidden_needed_libraries_on_executables() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("artifact");
        let bytes = elf64_executable_with_dynamic_strings(&[(DT_NEEDED, "libstdc++.so.6")]);
        fs::write(&path, bytes).unwrap();

        let missing = verify_tree(
            &path,
            "/pkgs/store/glibc/lib/ld-linux.so.2",
            &[],
            &["libgcc_s.so.1".to_owned()],
            &[],
        );
        assert!(matches!(missing, Err(Error::MissingNeededLibrary { .. })));

        let forbidden = verify_tree(
            &path,
            "/pkgs/store/glibc/lib/ld-linux.so.2",
            &[],
            &[],
            &["libstdc++.so.6".to_owned()],
        );
        assert!(matches!(
            forbidden,
            Err(Error::ForbiddenNeededLibrary { .. })
        ));
    }

    fn elf64_executable_with_dynamic_strings(entries: &[(u64, &str)]) -> Vec<u8> {
        let mut bytes = elf64_with_dynamic_strings(entries);
        let interpreter = b"/pkgs/store/glibc/lib/ld-linux.so.2\0";
        let offset = bytes.len();
        bytes.extend_from_slice(interpreter);
        bytes[56..58].copy_from_slice(&(3_u16).to_le_bytes());

        let interp = 64 + 2 * 56;
        bytes[interp..interp + 4].copy_from_slice(&PT_INTERP.to_le_bytes());
        bytes[interp + 8..interp + 16].copy_from_slice(&(offset as u64).to_le_bytes());
        bytes[interp + 32..interp + 40].copy_from_slice(&(interpreter.len() as u64).to_le_bytes());
        bytes
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

    fn elf64_with_dynamic_strings(entries: &[(u64, &str)]) -> Vec<u8> {
        let mut strings = vec![0];
        let mut offsets = Vec::new();
        for (tag, value) in entries {
            offsets.push((*tag, strings.len() as u64));
            strings.extend_from_slice(value.as_bytes());
            strings.push(0);
        }
        let mut bytes = vec![0; 400 + strings.len()];
        let byte_len = bytes.len() as u64;
        bytes[..4].copy_from_slice(ELF_MAGIC);
        bytes[4] = ELFCLASS64;
        bytes[5] = ELFDATA2LSB;
        bytes[32..40].copy_from_slice(&(64_u64).to_le_bytes());
        bytes[54..56].copy_from_slice(&(56_u16).to_le_bytes());
        bytes[56..58].copy_from_slice(&(2_u16).to_le_bytes());

        let load = 64;
        bytes[load..load + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[load + 16..load + 24].copy_from_slice(&(0x1000_u64).to_le_bytes());
        bytes[load + 32..load + 40].copy_from_slice(&byte_len.to_le_bytes());

        let dynamic = load + 56;
        bytes[dynamic..dynamic + 4].copy_from_slice(&PT_DYNAMIC.to_le_bytes());
        bytes[dynamic + 8..dynamic + 16].copy_from_slice(&(256_u64).to_le_bytes());
        bytes[dynamic + 16..dynamic + 24].copy_from_slice(&(0x1100_u64).to_le_bytes());
        bytes[dynamic + 32..dynamic + 40]
            .copy_from_slice(&(((entries.len() + 2) * 16) as u64).to_le_bytes());

        bytes[256..264].copy_from_slice(&DT_STRTAB.to_le_bytes());
        bytes[264..272].copy_from_slice(&(0x1000_u64 + 400).to_le_bytes());
        for (index, (tag, offset)) in offsets.iter().enumerate() {
            let start = 272 + index * 16;
            bytes[start..start + 8].copy_from_slice(&tag.to_le_bytes());
            bytes[start + 8..start + 16].copy_from_slice(&offset.to_le_bytes());
        }
        let null = 272 + offsets.len() * 16;
        bytes[null..null + 8].copy_from_slice(&DT_NULL.to_le_bytes());
        bytes[400..].copy_from_slice(&strings);
        bytes
    }
}
