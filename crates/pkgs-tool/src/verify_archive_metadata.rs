use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use thiserror::Error;

use crate::common;

const AR_MAGIC: &[u8] = b"!<arch>\n";
const AR_HEADER_LEN: usize = 60;
const GZIP_MAGIC: &[u8] = &[0x1f, 0x8b, 0x08];
const GZIP_HEADER_LEN: usize = 10;
const GZIP_NAME_FLAG: u8 = 0x08;

#[derive(Debug, Parser)]
#[command(name = "pkgs-verify-archive-metadata")]
pub(crate) struct Args {
    #[arg(long)]
    input: PathBuf,

    #[arg(long)]
    stamp: PathBuf,
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Common(#[from] common::Error),

    #[error("archive field {field} is not canonical at {path}")]
    NonCanonicalArchiveField { path: PathBuf, field: &'static str },

    #[error("archive is malformed at {path}: {message}")]
    MalformedArchive {
        path: PathBuf,
        message: &'static str,
    },

    #[error("gzip header keeps a non-zero timestamp at {path}: {timestamp}")]
    GzipTimestamp { path: PathBuf, timestamp: u32 },

    #[error("gzip header keeps the original filename at {0}")]
    GzipOriginalName(PathBuf),
}

pub(crate) fn run(args: &Args) -> Result<(), Error> {
    verify_tree(&args.input, Path::new("."))?;

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

fn verify_tree(path: &Path, relative: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| common::Error::Metadata {
        path: path.to_path_buf(),
        source,
    })?;

    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in common::sorted_dir_entries(path)? {
            verify_tree(&entry.path(), &relative.join(entry.file_name()))?;
        }
        return Ok(());
    }

    if !metadata.is_file() {
        return Ok(());
    }

    let bytes = fs::read(path).map_err(|source| common::Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    if bytes.starts_with(AR_MAGIC) {
        verify_ar_archive(relative, &bytes)?;
    }
    if bytes.starts_with(GZIP_MAGIC) {
        verify_gzip_header(relative, &bytes)?;
    }
    Ok(())
}

fn verify_ar_archive(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut cursor = AR_MAGIC.len();
    while cursor < bytes.len() {
        let header = bytes
            .get(cursor..cursor + AR_HEADER_LEN)
            .ok_or_else(|| malformed_archive(path, "truncated member header"))?;
        if &header[58..60] != b"`\n" {
            return Err(malformed_archive(path, "invalid member trailer"));
        }

        require_zero_decimal(path, "timestamp", &header[16..28])?;
        require_zero_decimal(path, "uid", &header[28..34])?;
        require_zero_decimal(path, "gid", &header[34..40])?;

        let payload_len = parse_decimal(path, "size", &header[48..58])?;
        cursor += AR_HEADER_LEN;
        cursor = cursor
            .checked_add(payload_len)
            .ok_or_else(|| malformed_archive(path, "member size overflow"))?;
        if cursor > bytes.len() {
            return Err(malformed_archive(path, "member payload extends past EOF"));
        }
        if payload_len % 2 == 1 {
            cursor = cursor
                .checked_add(1)
                .ok_or_else(|| malformed_archive(path, "member padding overflow"))?;
            if cursor > bytes.len() {
                return Err(malformed_archive(path, "member padding extends past EOF"));
            }
        }
    }

    Ok(())
}

fn verify_gzip_header(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let header = bytes
        .get(..GZIP_HEADER_LEN)
        .ok_or_else(|| malformed_archive(path, "truncated gzip header"))?;
    let timestamp = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    if timestamp != 0 {
        return Err(Error::GzipTimestamp {
            path: path.to_path_buf(),
            timestamp,
        });
    }
    if header[3] & GZIP_NAME_FLAG != 0 {
        return Err(Error::GzipOriginalName(path.to_path_buf()));
    }
    Ok(())
}

fn require_zero_decimal(path: &Path, field: &'static str, bytes: &[u8]) -> Result<(), Error> {
    if parse_decimal(path, field, bytes)? != 0 {
        return Err(Error::NonCanonicalArchiveField {
            path: path.to_path_buf(),
            field,
        });
    }
    Ok(())
}

fn parse_decimal(path: &Path, field: &'static str, bytes: &[u8]) -> Result<usize, Error> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| malformed_archive(path, "non-UTF-8 numeric field"))?
        .trim();
    if value.is_empty() {
        return Err(Error::NonCanonicalArchiveField {
            path: path.to_path_buf(),
            field,
        });
    }

    value
        .parse::<usize>()
        .map_err(|_| malformed_archive(path, "numeric archive field is not valid decimal"))
}

fn malformed_archive(path: &Path, message: &'static str) -> Error {
    Error::MalformedArchive {
        path: path.to_path_buf(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_archive_metadata() {
        let bytes = ar_archive("0", "0", "0");
        verify_ar_archive(Path::new("libexample.a"), &bytes).unwrap();
    }

    #[test]
    fn rejects_non_canonical_archive_timestamps() {
        let bytes = ar_archive("123", "0", "0");
        assert!(matches!(
            verify_ar_archive(Path::new("libexample.a"), &bytes),
            Err(Error::NonCanonicalArchiveField {
                field: "timestamp",
                ..
            })
        ));
    }

    #[test]
    fn accepts_canonical_gzip_headers() {
        verify_gzip_header(
            Path::new("manual.gz"),
            &[0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0],
        )
        .unwrap();
    }

    #[test]
    fn rejects_gzip_timestamps_and_original_names() {
        assert!(matches!(
            verify_gzip_header(
                Path::new("manual.gz"),
                &[0x1f, 0x8b, 0x08, 0, 1, 0, 0, 0, 0, 0],
            ),
            Err(Error::GzipTimestamp { timestamp: 1, .. })
        ));
        assert!(matches!(
            verify_gzip_header(
                Path::new("manual.gz"),
                &[0x1f, 0x8b, 0x08, GZIP_NAME_FLAG, 0, 0, 0, 0, 0, 0],
            ),
            Err(Error::GzipOriginalName(_))
        ));
    }

    fn ar_archive(timestamp: &str, uid: &str, gid: &str) -> Vec<u8> {
        let payload = b"obj\n";
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            "payload.o/",
            timestamp,
            uid,
            gid,
            "100644",
            payload.len(),
        );
        let mut archive = AR_MAGIC.to_vec();
        archive.extend_from_slice(header.as_bytes());
        archive.extend_from_slice(payload);
        archive
    }
}
