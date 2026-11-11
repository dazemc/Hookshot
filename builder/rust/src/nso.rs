//! Rewrite a ZBIC-compressed NSO (flags bit 7, Switch firmware 22.0.0+) as a
//! plain one, in place, so every later tool reads it unchanged. Ported from
//! nx2/unpack.py: the segment hashes cover the decompressed data and stay
//! valid, and they are checked here.

use std::fs;
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};

const ZBIC_FLAG: u32 = 1 << 7;

/// The module names an exefs may hold, in the order nup.py heals them.
pub const NAMES: [&str; 13] = [
    "rtld", "main", "subsdk0", "subsdk1", "subsdk2", "subsdk3", "subsdk4", "subsdk5", "subsdk6",
    "subsdk7", "subsdk8", "subsdk9", "sdk",
];

/// None: not an NSO. Some(false): already plain. Some(true): rewritten.
pub fn unpack_nso(path: &Path) -> Result<Option<bool>, String> {
    let raw = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if raw.len() < 0x100 || &raw[..4] != b"NSO0" {
        return Ok(None);
    }
    let flags = u32::from_le_bytes(raw[0x0c..0x10].try_into().unwrap());
    if flags & ZBIC_FLAG == 0 {
        return Ok(Some(false));
    }
    let mut segments = [(0u32, 0u32); 3];
    for (i, segment) in segments.iter_mut().enumerate() {
        let at = 0x10 + 0x10 * i;
        segment.0 = u32::from_le_bytes(raw[at..at + 4].try_into().unwrap());
        segment.1 = u32::from_le_bytes(raw[at + 8..at + 12].try_into().unwrap());
    }
    let mut data = Vec::with_capacity(3);
    for (i, (offset, size)) in segments.iter().copied().enumerate() {
        let compressed = u32::from_le_bytes(raw[0x60 + 4 * i..0x64 + 4 * i].try_into().unwrap());
        let start = offset as usize;
        let end = start
            .checked_add(compressed as usize)
            .filter(|end| *end <= raw.len())
            .ok_or_else(|| format!("{}: segment {i} outside the file", path.display()))?;
        let mut segment = raw[start..end].to_vec();
        if flags & (1 << i) != 0 {
            segment = crate::zbic::decompress(&segment, size as usize)
                .map_err(|e| format!("{}: segment {i}: {e}", path.display()))?;
        }
        if segment.len() != size as usize {
            return Err(format!(
                "{}: segment {i}: {} bytes, {size} expected",
                path.display(),
                segment.len()
            ));
        }
        if flags & (8 << i) != 0 {
            let at = 0xa0 + 0x20 * i;
            let want = &raw[at..at + 0x20];
            if Sha256::digest(&segment).as_slice() != want {
                return Err(format!(
                    "{}: segment {i}: hash mismatch after ZBIC",
                    path.display()
                ));
            }
        }
        data.push(segment);
    }
    let first = segments
        .iter()
        .map(|s| s.0 as usize)
        .min()
        .unwrap_or(0x100)
        .max(0x100);
    let mut header = raw[..0x100].to_vec();
    let mut body = raw[0x100..first.min(raw.len())].to_vec();
    for (i, segment) in data.iter().enumerate() {
        let at = 0x10 + 0x10 * i;
        header[at..at + 4].copy_from_slice(&(0x100 + body.len() as u32).to_le_bytes());
        let at = 0x60 + 4 * i;
        header[at..at + 4].copy_from_slice(&(segment.len() as u32).to_le_bytes());
        body.extend_from_slice(segment);
    }
    let new_flags = flags & !(ZBIC_FLAG | 7);
    header[0x0c..0x10].copy_from_slice(&new_flags.to_le_bytes());
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".plain");
    let temporary = std::path::PathBuf::from(temporary);
    {
        let mut file =
            fs::File::create(&temporary).map_err(|e| format!("{}: {e}", temporary.display()))?;
        file.write_all(&header)
            .map_err(|e| format!("{}: {e}", temporary.display()))?;
        file.write_all(&body)
            .map_err(|e| format!("{}: {e}", temporary.display()))?;
    }
    fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_a_plain_nso_alone() {
        let path = std::env::temp_dir().join(format!("hookshot-rust-nso-{}", std::process::id()));
        let mut bytes = vec![0u8; 0x100];
        bytes[..4].copy_from_slice(b"NSO0");
        fs::write(&path, &bytes).unwrap();
        assert_eq!(unpack_nso(&path).unwrap(), Some(false));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ignores_other_files() {
        let path =
            std::env::temp_dir().join(format!("hookshot-rust-not-nso-{}", std::process::id()));
        fs::write(&path, b"plain data").unwrap();
        assert_eq!(unpack_nso(&path).unwrap(), None);
        let _ = fs::remove_file(&path);
    }
}
