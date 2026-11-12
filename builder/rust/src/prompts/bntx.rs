//! The Switch 2 BNTX of the button icons: its texture table and the
//! block-linear (GOB) address of a block. `Common.bntx` is the game's file
//! decompressed; `textures.bin` writes BC4 blocks over the BC4 entries.
//!
//! Port of `nx2/prompt_assets.py`'s `bntx_textures` and `gob_offset`; the
//! ASTC entries are metadata only here (their pictures are not drawn yet).

use std::collections::BTreeMap;
pub const TEXTURES: &str = "UI/Layout2Archive/Common.Product.100.Nin_Ounce_NVN2.bntx.zs";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    pub name: String,
    pub record: u64,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    /// The bytes of one GOB in a block's rows (`1 << height_bits`).
    pub gobs: u32,
    pub data: u64,
}

impl Texture {
    /// 16 for ASTC 4x4, 8 for BC4.
    pub fn bytes_per_block(&self) -> usize {
        if self.format >> 8 == 0x2d { 16 } else { 8 }
    }

    pub fn is_astc(&self) -> bool {
        self.format >> 8 == 0x2d
    }

    pub fn blocks(&self) -> (u32, u32) {
        (self.width.div_ceil(4), self.height.div_ceil(4))
    }

    pub fn pitch(&self) -> u32 {
        (self.blocks().0 * self.bytes_per_block() as u32).div_ceil(64)
    }
}

fn u16_at(bytes: &[u8], at: u64) -> Option<u16> {
    let at = usize::try_from(at).ok()?;
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: u64) -> Option<u32> {
    let at = usize::try_from(at).ok()?;
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: u64) -> Option<u64> {
    let at = usize::try_from(at).ok()?;
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// `{name: texture}` of a Switch 2 BNTX (platform "Ounc", 64-bit pointers).
pub fn parse(bytes: &[u8]) -> Result<BTreeMap<String, Texture>, String> {
    if bytes.get(..4) != Some(b"BNTX") {
        return Err("bntx: not a BNTX file".to_owned());
    }
    let count = u32_at(bytes, 0x24).ok_or("bntx: no texture count")?;
    let info = u64_at(bytes, 0x28).ok_or("bntx: no texture table")?;
    let mut out = BTreeMap::new();
    for index in 0..u64::from(count) {
        let record = u64_at(bytes, info + index * 8)
            .ok_or_else(|| format!("bntx: texture {index} pointer is outside the file"))?;
        let name_at = u64_at(bytes, record + 0x60)
            .ok_or_else(|| format!("bntx: texture {index} has no name pointer"))?;
        let name_len = usize::from(u16_at(bytes, name_at).ok_or("bntx: short texture name")?);
        let name_at = usize::try_from(name_at + 2).map_err(|_| "bntx: name offset".to_owned())?;
        let name = bytes
            .get(name_at..name_at + name_len)
            .ok_or("bntx: texture name outside the file")?
            .split(|byte| *byte == 0)
            .next()
            .unwrap_or_default();
        let name = String::from_utf8_lossy(name).into_owned();
        let format = u32_at(bytes, record + 0x1c).ok_or("bntx: short texture format")?;
        let width = u32_at(bytes, record + 0x24).ok_or("bntx: short texture width")?;
        let height = u32_at(bytes, record + 0x28).ok_or("bntx: short texture height")?;
        let height_bits = u32_at(bytes, record + 0x34).ok_or("bntx: short texture layout")?;
        let mips = u64_at(bytes, record + 0x70).ok_or("bntx: short texture mips")?;
        let data = u64_at(bytes, mips).ok_or("bntx: short texture data")?;
        out.insert(
            name.clone(),
            Texture {
                name,
                record,
                format,
                width,
                height,
                gobs: 1u32 << height_bits.min(31),
                data,
            },
        );
    }
    Ok(out)
}

/// A byte's place in a block-linear texture: x in bytes, y in rows of blocks.
/// The Switch 2's GOB (Tegra X1's with offset bits 5 and 6 swapped, as
/// `gpu/render/rx_nvn.cpp`'s `bl_offset`).
pub fn gob_offset(x: u64, y: u64, gobs: u64, pitch: u64) -> u64 {
    let a =
        (y / (8 * gobs)) * 512 * gobs * pitch + (x / 64) * 512 * gobs + (y % (8 * gobs) / 8) * 512;
    a + (((x & 63) >> 5) << 8
        | ((y & 7) >> 2) << 7
        | ((x & 31) >> 4) << 6
        | ((y >> 1) & 1) << 5
        | (y & 1) << 4
        | (x & 15))
}

/// A texture's blocks in row-major block order, as ASTC/BC4 expects.
pub fn blocks_linear(bytes: &[u8], texture: &Texture) -> Vec<u8> {
    let (bw, bh) = texture.blocks();
    let size = texture.bytes_per_block();
    let mut out = Vec::with_capacity((bw * bh) as usize * size);
    for y in 0..bh {
        for x in 0..bw {
            if let Some(block) = block(bytes, texture, x, y) {
                out.extend_from_slice(block);
            } else {
                out.extend(std::iter::repeat_n(0, size));
            }
        }
    }
    out
}

/// The file's bytes at a texture's block (x, y), or None outside.
pub fn block<'a>(bytes: &'a [u8], texture: &Texture, x: u32, y: u32) -> Option<&'a [u8]> {
    let size = texture.bytes_per_block();
    let at = texture.data
        + gob_offset(
            u64::from(x) * size as u64,
            u64::from(y),
            u64::from(texture.gobs),
            u64::from(texture.pitch()),
        );
    let at = usize::try_from(at).ok()?;
    bytes.get(at..at + size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bntx(name: &str, format: u32, width: u32, height: u32, height_bits: u32) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x300];
        bytes[..4].copy_from_slice(b"BNTX");
        bytes[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
        bytes[0x28..0x30].copy_from_slice(&0x100u64.to_le_bytes());
        bytes[0x100..0x108].copy_from_slice(&0x140u64.to_le_bytes());
        let record = 0x140usize;
        bytes[record + 0x1c..record + 0x20].copy_from_slice(&format.to_le_bytes());
        bytes[record + 0x24..record + 0x28].copy_from_slice(&width.to_le_bytes());
        bytes[record + 0x28..record + 0x2c].copy_from_slice(&height.to_le_bytes());
        bytes[record + 0x34..record + 0x38].copy_from_slice(&height_bits.to_le_bytes());
        bytes[record + 0x60..record + 0x68].copy_from_slice(&0x1c0u64.to_le_bytes());
        bytes[record + 0x70..record + 0x78].copy_from_slice(&0x1b0u64.to_le_bytes());
        bytes[0x1c0..0x1c2].copy_from_slice(&(name.len() as u16).to_le_bytes());
        bytes[0x1c2..0x1c2 + name.len()].copy_from_slice(name.as_bytes());
        bytes[0x1b0..0x1b8].copy_from_slice(&0x1d0u64.to_le_bytes());
        bytes
    }

    #[test]
    fn reads_a_bc4_texture_and_an_astc_one() {
        let bc4 = parse(&test_bntx("KeyTexFontA_00", 0, 64, 32, 1)).unwrap();
        let t = &bc4["KeyTexFontA_00"];
        assert_eq!(t.bytes_per_block(), 8);
        assert_eq!(t.blocks(), (16, 8));
        assert_eq!(t.gobs, 2);
        assert!(!t.is_astc());

        let astc = parse(&test_bntx("OcarinaButtonA_00", 0x2d02, 64, 32, 0)).unwrap();
        assert!(astc["OcarinaButtonA_00"].is_astc());
        assert_eq!(astc["OcarinaButtonA_00"].bytes_per_block(), 16);
    }

    #[test]
    fn gob_addresses_match_the_console_layout() {
        // (one GOB wide, two high: 512 bytes a GOB; x moves by 64-byte blocks)
        assert_eq!(gob_offset(0, 0, 2, 1), 0);
        assert_eq!(gob_offset(64, 0, 2, 1), 1024);
        assert_eq!(gob_offset(0, 8, 2, 1), 512);
        assert_eq!(gob_offset(0, 16, 2, 1), 1024);
        assert_eq!(gob_offset(32, 0, 2, 1), 256);
        assert_eq!(gob_offset(0, 4, 2, 1), 128);
    }
}
