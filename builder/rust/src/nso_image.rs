//! NSO (Switch executable) loader: segments, MOD0, dynamic section, symbols,
//! relocations (RELA and Switch 2 RELR), init array and the unwind table.
//! Ported from a64-kit's tools/nso.py and nx2/nso2.py.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::lz4;

const MODULE_ORDER: [&str; 13] = [
    "rtld", "main", "subsdk0", "subsdk1", "subsdk2", "subsdk3", "subsdk4", "subsdk5", "subsdk6",
    "subsdk7", "subsdk8", "subsdk9", "sdk",
];

#[derive(Clone, Debug)]
pub struct Segment {
    pub offset: u64,
    pub size: u64,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub bind: String,
    pub defined: bool,
    pub value: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Reloc {
    pub offset: u64,
    pub kind: String,
    pub sym: Option<String>,
    pub addend: i64,
}

pub struct Nso {
    pub name: String,
    pub image: Vec<u8>,
    pub build_id: String,
    pub segments: Vec<Segment>,
    pub bss: (usize, usize),
    pub bss_size: usize,
    pub eh_frame_hdr: (usize, usize),
    pub mod0: usize,
    pub module_object: usize,
    pub dynamic: HashMap<String, Vec<u64>>,
    pub symbols: Vec<Symbol>,
    pub relocs: Vec<Reloc>,
    pub init_array: Vec<u64>,
    pub eh_funcs: Vec<usize>,
    pub relr_count: usize,
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn i32_at(data: &[u8], at: usize) -> Option<i32> {
    data.get(at..at + 4)
        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
}

fn c_string(data: &[u8], at: usize) -> String {
    let end = data
        .get(at..)
        .and_then(|rest| rest.iter().position(|byte| *byte == 0))
        .map(|n| at + n)
        .unwrap_or(data.len());
    String::from_utf8_lossy(&data[at.min(data.len())..end]).into_owned()
}

fn dt_name(tag: i64) -> String {
    match tag {
        1 => "NEEDED",
        2 => "PLTRELSZ",
        3 => "PLTGOT",
        4 => "HASH",
        5 => "STRTAB",
        6 => "SYMTAB",
        7 => "RELA",
        8 => "RELASZ",
        9 => "RELAENT",
        10 => "STRSZ",
        11 => "SYMENT",
        12 => "INIT",
        13 => "FINI",
        20 => "PLTREL",
        23 => "JMPREL",
        25 => "INIT_ARRAY",
        26 => "FINI_ARRAY",
        27 => "INIT_ARRAYSZ",
        28 => "FINI_ARRAYSZ",
        30 => "FLAGS",
        35 => "RELRSZ",
        36 => "RELR",
        37 => "RELRENT",
        21 => "DEBUG",
        0x6FFFFEF5 => "GNU_HASH",
        0x6FFFFFF9 => "RELACOUNT",
        0x6FFFFFFB => "FLAGS_1",
        _ => return tag.to_string(),
    }
    .to_owned()
}

fn r_type(value: u32) -> String {
    match value {
        257 => "ABS64",
        1025 => "GLOB_DAT",
        1026 => "JUMP_SLOT",
        1027 => "RELATIVE",
        1030 => "TLSDESC",
        _ => return value.to_string(),
    }
    .to_owned()
}

fn symbol_type(value: u8) -> String {
    match value {
        0 => "notype",
        1 => "object",
        2 => "func",
        3 => "section",
        4 => "file",
        6 => "tls",
        _ => return value.to_string(),
    }
    .to_owned()
}

fn symbol_bind(value: u8) -> String {
    match value {
        0 => "local",
        1 => "global",
        2 => "weak",
        _ => return value.to_string(),
    }
    .to_owned()
}

impl Nso {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if raw.len() < 0x100 || &raw[..4] != b"NSO0" {
            return Err(format!("{}: not an NSO", path.display()));
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let flags = u32_at(&raw, 0x0c).unwrap();
        let mut segments_raw = Vec::new();
        for i in 0..3 {
            let at = 0x10 + 0x10 * i;
            segments_raw.push((
                u32_at(&raw, at).unwrap() as usize,
                u32_at(&raw, at + 4).unwrap() as usize,
                u32_at(&raw, at + 8).unwrap() as usize,
            ));
        }
        let mut compressed = [0u32; 3];
        for (i, value) in compressed.iter_mut().enumerate() {
            *value = u32_at(&raw, 0x60 + 4 * i).unwrap();
        }
        let bss_size = u32_at(&raw, 0x3c).unwrap() as usize;
        let id = &raw[0x40..0x60];
        let end = id
            .iter()
            .rposition(|byte| *byte != 0)
            .map(|at| at + 1)
            .unwrap_or(0);
        let build_id = id[..end].iter().map(|byte| format!("{byte:02x}")).collect();
        let mut segments = Vec::new();
        let mut image = Vec::new();
        for (i, (foff, moff, size)) in segments_raw.iter().copied().enumerate() {
            let end = foff
                .checked_add(compressed[i] as usize)
                .ok_or("segment overflows")?;
            let bytes = raw
                .get(foff..end)
                .ok_or_else(|| format!("{name} segment {i} outside the file"))?;
            let data = if flags & (1 << i) != 0 {
                lz4::decompress(bytes, size).map_err(|e| format!("{name} segment {i}: {e}"))?
            } else {
                if bytes.len() != size {
                    return Err(format!(
                        "{name} segment {i}: {} bytes, {size} expected",
                        bytes.len()
                    ));
                }
                bytes.to_vec()
            };
            if image.len() < moff + size {
                image.resize(moff + size, 0);
            }
            image[moff..moff + size].copy_from_slice(&data);
            segments.push(Segment {
                offset: moff as u64,
                size: size as u64,
                name: [".text", ".rodata", ".data"][i].to_owned(),
            });
        }
        let mut nso = Self {
            name,
            image,
            build_id,
            segments,
            bss: (0, 0),
            bss_size,
            eh_frame_hdr: (0, 0),
            mod0: 0,
            module_object: 0,
            dynamic: HashMap::new(),
            symbols: Vec::new(),
            relocs: Vec::new(),
            init_array: Vec::new(),
            eh_funcs: Vec::new(),
            relr_count: 0,
        };
        nso.read_mod0(bss_size)?;
        nso.read_relr()?;
        Ok(nso)
    }

    fn u32(&self, at: usize) -> u32 {
        u32_at(&self.image, at).unwrap_or(0)
    }

    fn u64(&self, at: usize) -> u64 {
        u64_at(&self.image, at).unwrap_or(0)
    }

    fn d(&self, key: &str) -> u64 {
        self.dynamic
            .get(key)
            .and_then(|values| values.first())
            .copied()
            .unwrap_or(0)
    }

    fn read_mod0(&mut self, bss_size: usize) -> Result<(), String> {
        let m = self.u32(4) as usize;
        if self.image.get(m..m + 4) != Some(b"MOD0") {
            return Err(format!("{}: no MOD0", self.name));
        }
        self.mod0 = m;
        let dyn_off = i32_at(&self.image, m + 4).ok_or("short MOD0")? as i64;
        let bss_s = i32_at(&self.image, m + 8).ok_or("short MOD0")? as i64;
        let bss_e = i32_at(&self.image, m + 12).ok_or("short MOD0")? as i64;
        let eh_s = i32_at(&self.image, m + 16).ok_or("short MOD0")? as i64;
        let eh_e = i32_at(&self.image, m + 20).ok_or("short MOD0")? as i64;
        let modobj = i32_at(&self.image, m + 24).ok_or("short MOD0")? as i64;
        self.bss = ((m as i64 + bss_s) as usize, (m as i64 + bss_e) as usize);
        self.eh_frame_hdr = ((m as i64 + eh_s) as usize, (m as i64 + eh_e) as usize);
        self.module_object = (m as i64 + modobj) as usize;
        if self.image.len() < self.bss.1 {
            self.image.resize(self.bss.1, 0);
        } else if bss_size > self.bss.1.saturating_sub(self.bss.0) {
            self.image.resize(self.bss.0 + bss_size, 0);
        }
        let mut at = (m as i64 + dyn_off) as usize;
        loop {
            let tag = i64_at(&self.image, at).ok_or("short dynamic section")?;
            let value = u64_at(&self.image, at + 8).ok_or("short dynamic section")?;
            at += 16;
            if tag == 0 {
                break;
            }
            self.dynamic.entry(dt_name(tag)).or_default().push(value);
        }
        let strtab = self.d("STRTAB") as usize;
        let symtab = self.d("SYMTAB") as usize;
        let syment = (self.d("SYMENT") as usize).max(24);
        let nsyms = if strtab > symtab {
            (strtab - symtab) / syment
        } else {
            0
        };
        for i in 0..nsyms {
            let at = symtab + i * syment;
            let Some(name) = u32_at(&self.image, at) else {
                break;
            };
            let Some(info) = self.image.get(at + 4).copied() else {
                break;
            };
            let Some(shndx) = u16_at(&self.image, at + 6) else {
                break;
            };
            let value = self.u64(at + 8);
            let size = self.u64(at + 16);
            self.symbols.push(Symbol {
                name: c_string(&self.image, strtab + name as usize),
                kind: symbol_type(info & 0xf),
                bind: symbol_bind(info >> 4),
                defined: shndx != 0,
                value,
                size,
            });
        }
        for (start_key, size_key) in [("RELA", "RELASZ"), ("JMPREL", "PLTRELSZ")] {
            let start = self.d(start_key) as usize;
            let size = self.d(size_key) as usize;
            for off in (start..start + size).step_by(24) {
                let Some(r_off) = u64_at(&self.image, off) else {
                    break;
                };
                let Some(r_info) = u64_at(&self.image, off + 8) else {
                    break;
                };
                let Some(addend) = i64_at(&self.image, off + 16) else {
                    break;
                };
                let sym = (r_info >> 32) as usize;
                self.relocs.push(Reloc {
                    offset: r_off,
                    kind: r_type(r_info as u32),
                    sym: if sym == 0 {
                        None
                    } else {
                        self.symbols.get(sym).map(|symbol| symbol.name.clone())
                    },
                    addend,
                });
            }
        }
        let init = self.d("INIT_ARRAY") as usize;
        let init_size = self.d("INIT_ARRAYSZ") as usize;
        self.init_array = (0..init_size / 8)
            .filter_map(|i| u64_at(&self.image, init + 8 * i))
            .collect();
        self.eh_funcs = self.read_eh_frame();
        Ok(())
    }

    fn read_eh_frame(&self) -> Vec<usize> {
        let m = self.mod0 as i64;
        let s = (m + i32_at(&self.image, self.mod0 + 16).unwrap_or(0) as i64) as usize;
        let e = (m + i32_at(&self.image, self.mod0 + 20).unwrap_or(0) as i64) as usize;
        if e <= s
            || self
                .image
                .get(s..s + 4)
                .map(|head| head[0] == 1 && head[2] == 0x03 && head[3] == 0x3B)
                .unwrap_or(false)
                == false
        {
            return Vec::new();
        }
        let ptr_enc = self.image[s + 1];
        if !matches!(ptr_enc & 0x0f, 0x0b | 0x03) {
            return Vec::new();
        }
        let count = self.u32(s + 8) as usize;
        (0..count)
            .filter_map(|i| {
                i32_at(&self.image, s + 12 + 8 * i).map(|rel| (s as i64 + rel as i64) as usize)
            })
            .collect()
    }

    /// Switch 2 packs relative relocations as RELR: an even entry is one
    /// address, an odd one a bitmap of 63 addresses after the cursor.
    fn read_relr(&mut self) -> Result<(), String> {
        let start = self.d("RELR") as usize;
        let size = self.d("RELRSZ") as usize;
        self.relr_count = 0;
        if start == 0 || size == 0 {
            return Ok(());
        }
        if !matches!(self.d("RELRENT"), 0 | 8) {
            return Err(format!("{}: RELRENT {}", self.name, self.d("RELRENT")));
        }
        let mut offsets = Vec::new();
        let mut where_ = 0u64;
        for at in (start..start + size).step_by(8) {
            let Some(entry) = u64_at(&self.image, at) else {
                break;
            };
            if entry & 1 == 0 {
                offsets.push(entry);
                where_ = entry.saturating_add(8);
            } else {
                let mut bits = entry >> 1;
                let mut i = 0u64;
                while bits != 0 {
                    if bits & 1 != 0 {
                        offsets.push(where_ + 8 * i);
                    }
                    bits >>= 1;
                    i += 1;
                }
                where_ = where_.saturating_add(63 * 8);
            }
        }
        for offset in offsets {
            let addend = u64_at(&self.image, offset as usize).unwrap_or(0) as i64;
            self.relocs.push(Reloc {
                offset,
                kind: "RELATIVE".into(),
                sym: None,
                addend,
            });
            self.relr_count += 1;
        }
        Ok(())
    }

    pub fn summary(&self) -> Value {
        let mut reloc_counts: HashMap<&str, usize> = HashMap::new();
        for reloc in &self.relocs {
            *reloc_counts.entry(&reloc.kind).or_default() += 1;
        }
        let defined = self
            .symbols
            .iter()
            .filter(|symbol| symbol.defined && !symbol.name.is_empty())
            .count();
        let imported = self
            .symbols
            .iter()
            .filter(|symbol| !symbol.defined && !symbol.name.is_empty())
            .count();
        json!({
            "name": self.name,
            "build_id": self.build_id,
            "image": self.image.len(),
            "segments": self.segments.iter().map(|segment| json!({"name": segment.name, "offset": segment.offset, "size": segment.size})).collect::<Vec<_>>(),
            "bss": self.bss,
            "symbols": self.symbols.len(),
            "defined": defined,
            "imported": imported,
            "relocs": self.relocs.len(),
            "reloc_kinds": reloc_counts,
            "init_array": self.init_array.len(),
            "eh_funcs": self.eh_funcs.len(),
            "relr": self.relr_count,
        })
    }

    pub fn image_path(&self, out: &Path) -> PathBuf {
        out.join(format!("{}.bin", self.name))
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn i64_at(data: &[u8], at: usize) -> Option<i64> {
    data.get(at..at + 8)
        .map(|bytes| i64::from_le_bytes(bytes.try_into().unwrap()))
}

pub fn load_exefs(path: &Path) -> Result<Vec<Nso>, String> {
    if path.is_file() {
        return Ok(vec![Nso::load(path)?]);
    }
    let mut out = Vec::new();
    for name in MODULE_ORDER {
        let file = path.join(name);
        if file.is_file() {
            out.push(Nso::load(&file)?);
        }
    }
    Ok(out)
}

pub fn json(exefs: &str) -> Result<Value, String> {
    let modules = load_exefs(Path::new(exefs))?;
    Ok(json!({
        "modules": modules.iter().map(Nso::summary).collect::<Vec<_>>(),
    }))
}

/// Write each module's loaded image, as nso.py's out_dir does.
pub fn dump_json(exefs: &str, out: &str) -> Result<Value, String> {
    let modules = load_exefs(Path::new(exefs))?;
    let out = Path::new(out);
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for module in &modules {
        fs::write(module.image_path(out), &module.image)
            .map_err(|e| format!("{}: {e}", module.image_path(out).display()))?;
    }
    Ok(json!({
        "out": out.to_string_lossy(),
        "modules": modules.iter().map(Nso::summary).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relr_entries_become_relative_relocations() {
        let mut image = vec![0u8; 0x200];
        image[0x100..0x108].copy_from_slice(&0x1234u64.to_le_bytes());
        image[0x108..0x110].copy_from_slice(&0x5678u64.to_le_bytes());
        // one address entry, then one bitmap entry selecting the next two
        image[0x20..0x28].copy_from_slice(&0x100u64.to_le_bytes());
        image[0x28..0x30].copy_from_slice(&0b1011u64.to_le_bytes());
        let mut nso = Nso {
            name: "test".into(),
            image,
            build_id: String::new(),
            segments: Vec::new(),
            bss: (0, 0),
            bss_size: 0,
            eh_frame_hdr: (0, 0),
            mod0: 0,
            module_object: 0,
            dynamic: HashMap::from([("RELR".into(), vec![0x20]), ("RELRSZ".into(), vec![16])]),
            symbols: Vec::new(),
            relocs: Vec::new(),
            init_array: Vec::new(),
            eh_funcs: Vec::new(),
            relr_count: 0,
        };
        nso.read_relr().unwrap();
        assert_eq!(nso.relr_count, 3);
        assert_eq!(nso.relocs[0].offset, 0x100);
        assert_eq!(nso.relocs[0].addend, 0x1234);
        assert_eq!(nso.relocs[1].offset, 0x108);
        assert_eq!(nso.relocs[2].offset, 0x118);
    }
}
