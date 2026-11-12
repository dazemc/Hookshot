//! The game's resource size table: `System/Resource/ResourceSizeTable.*`
//! (RESTBL, u32 version, u32 name length, u32 hashed entries, u32 named
//! entries; every hashed entry is a u32 CRC-32 of the path and its u32 size).
//! The External font is larger than the game's, and the game drops a file
//! larger than its entry.

use std::fs;
use std::path::Path;

use super::zs;

/// The font whose entry is raised, without the `.zs` the size table names.
pub const FONT_NO_SUFFIX: &str = "Font2/Scalable/External.Nin_Ounce_NVN2.bfotf";

/// The game's table with the font's entry raised to fit `font_size` bytes
/// from `game_size`: `None` when it fits already.
pub fn size_table(
    romfs: &Path,
    dictionary: &[u8],
    game_size: usize,
    font_size: usize,
) -> Result<Option<Vec<u8>>, String> {
    let folder = romfs.join("System").join("Resource");
    let mut names: Vec<String> = fs::read_dir(&folder)
        .map_err(|e| format!("prompts: no {}/: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("ResourceSizeTable.") && name.ends_with(".rsizetable.zs"))
        .collect();
    names.sort();
    let name = names.into_iter().next().ok_or_else(|| {
        "prompts: no System/Resource/ResourceSizeTable.*.rsizetable.zs".to_string()
    })?;
    let mut table = zs::read_zs(romfs, &format!("System/Resource/{name}"), dictionary)?;
    adjust(&mut table, game_size, font_size)
        .map(|changed| changed.map(|_| table))
        .map_err(|error| format!("prompts: {name}: {error}"))
}

fn adjust(table: &mut [u8], game_size: usize, font_size: usize) -> Result<Option<()>, String> {
    if table.len() < 22 || &table[..6] != b"RESTBL" {
        return Err("not a RESTBL table".into());
    }
    let hashed = u32::from_le_bytes(table[14..18].try_into().unwrap()) as usize;
    let want = crc32fast::hash(FONT_NO_SUFFIX.as_bytes());
    for i in 0..hashed {
        let at = 22 + 8 * i;
        let entry = table
            .get(at..at + 8)
            .ok_or_else(|| "the hashed entries run past the table".to_string())?;
        let hash = u32::from_le_bytes(entry[..4].try_into().unwrap());
        let size = u32::from_le_bytes(entry[4..].try_into().unwrap());
        if hash == want {
            let fit = font_size as u64 + size as u64 - game_size as u64;
            if fit <= size as u64 {
                return Ok(None);
            }
            table[at + 4..at + 8].copy_from_slice(&(fit as u32).to_le_bytes());
            return Ok(Some(()));
        }
    }
    Err(format!("has no entry for {FONT_NO_SUFFIX}"))
}

/// The table as the size table file is written: a zstd frame, level 3.
pub fn compress(table: &[u8]) -> Result<Vec<u8>, String> {
    zstd::bulk::compress(table, 3).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_the_usual_ieee_one() {
        assert_eq!(crc32fast::hash(b"123456789"), 0xCBF4_3926);
    }

    fn table(entry_size: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"RESTBL");
        b.extend_from_slice(&1u32.to_le_bytes()); // version
        b.extend_from_slice(&0u32.to_le_bytes()); // name length
        b.extend_from_slice(&2u32.to_le_bytes()); // hashed entries
        b.extend_from_slice(&0u32.to_le_bytes()); // named entries
        b.extend_from_slice(&0u32.to_le_bytes()); // another file's hash
        b.extend_from_slice(&100u32.to_le_bytes()); // and size
        b.extend_from_slice(&crc32fast::hash(FONT_NO_SUFFIX.as_bytes()).to_le_bytes());
        b.extend_from_slice(&entry_size.to_le_bytes());
        b
    }

    #[test]
    fn a_larger_font_raises_its_entry_by_the_difference() {
        let mut b = table(1200);
        assert_eq!(adjust(&mut b, 1000, 1266), Ok(Some(())));
        assert_eq!(u32::from_le_bytes(b[34..38].try_into().unwrap()), 1466);
    }

    #[test]
    fn a_font_that_fits_is_left_alone() {
        let mut b = table(1200);
        let before = b.clone();
        assert_eq!(adjust(&mut b, 1200, 1100), Ok(None));
        assert_eq!(b, before);
    }

    #[test]
    fn a_missing_entry_is_an_error() {
        let mut b = table(1200);
        b[30..34].copy_from_slice(&0u32.to_le_bytes());
        assert!(adjust(&mut b, 1000, 1200).is_err());
    }

    #[test]
    fn the_table_is_written_as_compressed_bytes() {
        let table = table(1200);
        let packed = compress(&table).unwrap();
        let plain = zstd::stream::decode_all(packed.as_slice()).unwrap();
        assert_eq!(plain, table);
    }
}
