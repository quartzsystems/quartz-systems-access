// Copyright (C), 2026 Quartz Systems. Some rights reserved. This work is
// licensed under the terms of the MIT license which can be found in the
// root directory of this project.

//! The installer file: the `qsa-install` program, then the payload (a
//! zstd-compressed tar of the files to install), then a footer:
//!
//! | Bytes | What |
//! |---|---|
//! | 8 | magic, `QSAPAYL1` |
//! | 8 | payload offset from the start of the file, little-endian |
//! | 8 | payload length, little-endian |
//! | 32 | SHA-256 of the payload |
//!
//! The hash is checked before anything is installed or changed.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Footer magic.
pub const MAGIC: &[u8; 8] = b"QSAPAYL1";
/// Footer length.
pub const FOOTER_LEN: u64 = 8 + 8 + 8 + 32;

/// Where the payload is in the installer file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Footer {
    pub offset: u64,
    pub len: u64,
    pub sha256: [u8; 32],
}

/// Why the payload cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("the file has no payload")]
    Missing,
    #[error("the payload does not match its checksum")]
    Damaged,
}

impl Footer {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(FOOTER_LEN as usize);
        v.extend_from_slice(MAGIC);
        v.extend_from_slice(&self.offset.to_le_bytes());
        v.extend_from_slice(&self.len.to_le_bytes());
        v.extend_from_slice(&self.sha256);
        v
    }

    pub fn decode(b: &[u8]) -> Option<Footer> {
        if b.len() != FOOTER_LEN as usize || &b[..8] != MAGIC {
            return None;
        }
        let u = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().expect("eight bytes"));
        Some(Footer {
            offset: u(8),
            len: u(16),
            sha256: b[24..56].try_into().expect("32 bytes"),
        })
    }
}

/// Reads and checks the footer of the installer at `path`: the payload must
/// lie inside the file and hash to the footer's checksum.
pub fn verify(path: &Path) -> Result<Footer, PayloadError> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    if size < FOOTER_LEN {
        return Err(PayloadError::Missing);
    }
    f.seek(SeekFrom::Start(size - FOOTER_LEN))?;
    let mut b = vec![0; FOOTER_LEN as usize];
    f.read_exact(&mut b)?;
    let footer = Footer::decode(&b).ok_or(PayloadError::Missing)?;
    let end = footer.offset.checked_add(footer.len);
    if end.is_none_or(|e| e > size - FOOTER_LEN) {
        return Err(PayloadError::Damaged);
    }
    f.seek(SeekFrom::Start(footer.offset))?;
    let mut h = Sha256::new();
    io::copy(&mut (&mut f).take(footer.len), &mut h)?;
    if h.finalize().as_slice() != footer.sha256 {
        return Err(PayloadError::Damaged);
    }
    Ok(footer)
}

/// The files of a payload, in tar order: (path inside, unpacked size).
pub fn list(path: &Path, footer: &Footer) -> Result<Vec<(PathBuf, u64)>, PayloadError> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(footer.offset))?;
    let dec = zstd::stream::read::Decoder::new(f.take(footer.len))?;
    let mut tar = tar::Archive::new(dec);
    let mut out = Vec::new();
    for e in tar.entries()? {
        let e = e?;
        if e.header().entry_type().is_file() {
            out.push((e.path()?.into_owned(), e.header().size()?));
        }
    }
    Ok(out)
}

/// Unpacks every file of the payload under `dir`, calling `each` with the
/// path inside the payload and where it was written. Paths that would leave
/// `dir` are refused.
pub fn extract(
    path: &Path,
    footer: &Footer,
    dir: &Path,
    mut each: impl FnMut(&Path, &Path),
) -> Result<(), PayloadError> {
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(footer.offset))?;
    let dec = zstd::stream::read::Decoder::new(f.take(footer.len))?;
    let mut tar = tar::Archive::new(dec);
    for e in tar.entries()? {
        let mut e = e?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let inner = e.path()?.into_owned();
        if inner
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(PayloadError::Damaged);
        }
        let to = dir.join(&inner);
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = to.with_extension("qsa-new");
        {
            let mut out = File::create(&tmp)?;
            io::copy(&mut e, &mut out)?;
            out.sync_all()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = e.header().mode().unwrap_or(0o644) & 0o755;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
        }
        replace(&tmp, &to)?;
        each(&inner, &to);
    }
    Ok(())
}

/// Moves `from` over `to`. On Windows a file in use cannot be replaced, but
/// it can be renamed: the old file is moved aside first and removed when
/// it can be.
pub fn replace(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if cfg!(windows) && to.exists() => {
            let aside = to.with_extension(format!("qsa-old-{}", std::process::id()));
            std::fs::rename(to, &aside).map_err(|_| e)?;
            std::fs::rename(from, to)?;
            let _ = std::fs::remove_file(&aside);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// One file to pack: its path inside the payload, its contents and whether
/// it is executable.
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
    pub executable: bool,
}

/// The payload for `entries`: a tar, compressed with zstd.
pub fn pack(entries: &[Entry]) -> io::Result<Vec<u8>> {
    let mut tar = tar::Builder::new(Vec::new());
    for e in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(e.data.len() as u64);
        h.set_mode(if e.executable { 0o755 } else { 0o644 });
        h.set_mtime(0);
        h.set_entry_type(tar::EntryType::Regular);
        tar.append_data(&mut h, &e.path, e.data.as_slice())?;
    }
    let raw = tar.into_inner()?;
    zstd::stream::encode_all(raw.as_slice(), 19)
}

/// The installer file: `stub`, then `payload`, then the footer.
pub fn assemble(stub: &[u8], payload: &[u8]) -> Vec<u8> {
    let footer = Footer {
        offset: stub.len() as u64,
        len: payload.len() as u64,
        sha256: Sha256::digest(payload).into(),
    };
    let mut out = Vec::with_capacity(stub.len() + payload.len() + FOOTER_LEN as usize);
    out.extend_from_slice(stub);
    out.extend_from_slice(payload);
    out.extend_from_slice(&footer.encode());
    out
}

/// Writes `bytes` to `path` and, on Unix, makes it executable.
pub fn write_installer(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qsa-payload-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample() -> Vec<Entry> {
        vec![
            Entry {
                path: "qsaccess".into(),
                data: b"\x7fELF program".to_vec(),
                executable: true,
            },
            Entry {
                path: "icons/hicolor/16x16/apps/quartz-access.png".into(),
                data: vec![1, 2, 3],
                executable: false,
            },
        ]
    }

    #[test]
    fn round_trip_and_damage_is_caught() {
        let dir = temp("rt");
        let file = dir.join("installer");
        let bytes = assemble(b"STUB-PROGRAM", &pack(&sample()).unwrap());
        write_installer(&file, &bytes).unwrap();
        let footer = verify(&file).unwrap();
        assert_eq!(footer.offset, 12);
        let names: Vec<String> = list(&file, &footer)
            .unwrap()
            .into_iter()
            .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            ["qsaccess", "icons/hicolor/16x16/apps/quartz-access.png"]
        );
        let out = dir.join("out");
        let mut seen = Vec::new();
        extract(&file, &footer, &out, |inner, _| seen.push(inner.to_owned())).unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            std::fs::read(out.join("qsaccess")).unwrap(),
            b"\x7fELF program"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(out.join("qsaccess"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(m & 0o777, 0o755);
        }

        // One byte changed in the payload.
        let mut bad = bytes.clone();
        bad[20] ^= 0xFF;
        write_installer(&file, &bad).unwrap();
        assert!(matches!(verify(&file), Err(PayloadError::Damaged)));
        // Cut short.
        write_installer(&file, &bytes[..bytes.len() - 10]).unwrap();
        assert!(matches!(verify(&file), Err(PayloadError::Missing)));
        // No payload at all.
        write_installer(&file, b"just a program").unwrap();
        assert!(matches!(verify(&file), Err(PayloadError::Missing)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn footer_round_trip() {
        let f = Footer {
            offset: 1234,
            len: 99,
            sha256: [7; 32],
        };
        assert_eq!(Footer::decode(&f.encode()), Some(f));
        assert_eq!(Footer::decode(&[0; 56]), None);
    }
}
