//! The game's zstd files: the dictionary out of `Pack/ZsDic.pack.zs` (a SARC)
//! and the files compressed with it.

use std::fs;
use std::io::Read;
use std::path::Path;

/// `zs.zsdic`, out of `Pack/ZsDic.pack.zs` (a SARC, itself zstd without a
/// dictionary). The SARC's data offset, hash table and name table are the
/// values `nx2/prompt_assets.py` reads.
pub fn zs_dictionary(romfs: &Path) -> Result<Vec<u8>, String> {
    let path = romfs.join("Pack").join("ZsDic.pack.zs");
    let packed = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut sarc = Vec::new();
    zstd::stream::read::Decoder::new(packed.as_slice())
        .map_err(|e| format!("{}: {e}", path.display()))?
        .read_to_end(&mut sarc)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    sarc_dictionary(&sarc).ok_or_else(|| "prompts: no zs.zsdic in Pack/ZsDic.pack.zs".into())
}

/// Find a named file in an uncompressed SARC. The name is stored at
/// `attr & 0xFFFFFF` words into the SFNT name table.
pub fn sarc_dictionary(sarc: &[u8]) -> Option<Vec<u8>> {
    if sarc.len() < 0x20 || &sarc[0..4] != b"SARC" {
        return None;
    }
    let header = u16::from_le_bytes(sarc[4..6].try_into().ok()?) as usize;
    let data = u32::from_le_bytes(sarc[12..16].try_into().ok()?) as usize;
    let sfat = header;
    if sarc.len() < sfat + 12 || &sarc[sfat..sfat + 4] != b"SFAT" {
        return None;
    }
    let count = u16::from_le_bytes(sarc[sfat + 6..sfat + 8].try_into().ok()?) as usize;
    let names = sfat + 12 + 16 * count + 8;
    for i in 0..count {
        let at = sfat + 12 + 16 * i;
        let attr = u32::from_le_bytes(sarc.get(at + 4..at + 8)?.try_into().ok()?);
        let start = u32::from_le_bytes(sarc.get(at + 8..at + 12)?.try_into().ok()?) as usize;
        let end = u32::from_le_bytes(sarc.get(at + 12..at + 16)?.try_into().ok()?) as usize;
        let offset = names.checked_add((attr & 0x00FF_FFFF) as usize * 4)?;
        let rest = sarc.get(offset..)?;
        let length = rest.iter().position(|b| *b == 0)?;
        if &rest[..length] == b"zs.zsdic" {
            let from = data.checked_add(start)?;
            let to = data.checked_add(end)?;
            return sarc.get(from..to).map(<[u8]>::to_vec);
        }
    }
    None
}

/// A `.zs` file decompressed with the game's dictionary.
pub fn read_zs(romfs: &Path, relative: &str, dictionary: &[u8]) -> Result<Vec<u8>, String> {
    let path = romfs.join(relative);
    let packed = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    read_zs_bytes(&packed, dictionary).map_err(|e| format!("{}: {e}", path.display()))
}

/// Decompress bytes that are a `.zs` frame made with `dictionary`.
pub fn read_zs_bytes(packed: &[u8], dictionary: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    zstd::stream::read::Decoder::with_dictionary(packed, dictionary)
        .map_err(|e| e.to_string())?
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sarc(name: &str, file: &[u8]) -> Vec<u8> {
        // (one node: SFAT at 0x20, SFNT at 0x20 + 12 + 16, names at + 8)
        let header = 0x20usize;
        let sfat = header;
        let names = sfat + 12 + 16 + 8;
        let data = names + 0x20;
        let name_at = 0usize;
        let mut out = vec![0u8; data + file.len()];
        out[0..4].copy_from_slice(b"SARC");
        out[4..6].copy_from_slice(&(header as u16).to_le_bytes());
        out[8..12].copy_from_slice(&(data as u32 + file.len() as u32).to_le_bytes());
        out[12..16].copy_from_slice(&(data as u32).to_le_bytes());
        out[sfat..sfat + 4].copy_from_slice(b"SFAT");
        out[sfat + 6..sfat + 8].copy_from_slice(&1u16.to_le_bytes());
        let node = sfat + 12;
        out[node + 4..node + 8].copy_from_slice(&((name_at as u32) & 0x00FF_FFFF).to_le_bytes());
        out[node + 8..node + 12].copy_from_slice(&0u32.to_le_bytes());
        out[node + 12..node + 16].copy_from_slice(&(file.len() as u32).to_le_bytes());
        out[sfat + 12 + 16..sfat + 12 + 20].copy_from_slice(b"SFNT");
        out[names..names + name.len()].copy_from_slice(name.as_bytes());
        out[data..].copy_from_slice(file);
        out
    }

    #[test]
    fn the_dictionary_is_found_in_a_sarc() {
        let bytes = sarc("zs.zsdic", b"a dictionary");
        assert_eq!(
            sarc_dictionary(&bytes).as_deref(),
            Some(&b"a dictionary"[..])
        );
    }

    #[test]
    fn another_name_is_not_the_dictionary() {
        let bytes = sarc("other.bin", b"nope");
        assert!(sarc_dictionary(&bytes).is_none());
        assert!(sarc_dictionary(b"").is_none());
    }

    #[test]
    fn a_dictionary_compressed_file_reads_back() {
        let dictionary = b"the game's zstd dictionary";
        let text = b"the font, decompressed with the dictionary";
        let packed = zstd::bulk::Compressor::with_dictionary(3, dictionary)
            .unwrap()
            .compress(text)
            .unwrap();
        assert_eq!(read_zs_bytes(&packed, dictionary).unwrap(), text);
    }
}
