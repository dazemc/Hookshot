//! The guest image at the addresses Eden loads it to (nx2/image.py): rtld,
//! main and the subsdks back to back, relocations applied and imports bound.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::nso_image::{self, Nso};

const PAGE: u64 = 0x1000;
const CODE_39BIT: u64 = 0x8000_0000;
const ARGS_SIZE: u64 = 0x9000;
const TRAMP_BASE: u64 = 0x0700_0000;

fn page_align(value: u64) -> u64 {
    (value + PAGE - 1) & !(PAGE - 1)
}

fn c_ident(symbol: &str) -> String {
    let mut out = String::from("imp_");
    for c in symbol.chars() {
        out.push(if c.is_ascii_alphanumeric() || c == '_' {
            c
        } else {
            '_'
        });
    }
    out
}

struct Placed {
    nso: Nso,
    base: u64,
}

pub struct Image {
    base: u64,
    end: u64,
    mem: Vec<u8>,
    modules: Vec<Placed>,
    hle: HashSet<String>,
    exports: HashMap<String, (usize, usize)>,
    pub hooks: BTreeMap<u64, String>,
    hook_syms: HashMap<String, String>,
    pub tramp: HashMap<String, u64>,
    pub unresolved: BTreeMap<String, usize>,
    pub entries: HashSet<u64>,
    init_arrays: BTreeMap<String, Vec<u64>>,
    eh_starts: Vec<u64>,
    names: BTreeMap<u64, String>,
    skipped: Vec<String>,
    code: Vec<(u64, u64, String)>,
}

fn eden_size(module: &Nso) -> u64 {
    let data = module
        .segments
        .get(2)
        .map(|segment| segment.offset + segment.size)
        .unwrap_or(0);
    page_align(data + module.bss_size as u64)
}

impl Image {
    pub fn build(
        exefs: &Path,
        aslr: u64,
        program_args: bool,
        hle: &[String],
        skip: &[String],
    ) -> Result<Self, String> {
        let base = CODE_39BIT + aslr;
        let skip: HashSet<&str> = skip.iter().map(String::as_str).collect();
        let mut modules = Vec::new();
        let mut at = base;
        let mut skipped = Vec::new();
        for mut nso in nso_image::load_exefs(exefs)? {
            if skip.contains(nso.name.as_str()) {
                skipped.push(nso.name.clone());
                continue;
            }
            let size = eden_size(&nso)
                + if nso.name == "rtld" && program_args {
                    ARGS_SIZE
                } else {
                    0
                };
            if nso.image.len() < size as usize {
                nso.image.resize(size as usize, 0);
            }
            modules.push(Placed { nso, base: at });
            at += size;
        }
        let end = at;
        let mut mem = vec![0u8; (end - base) as usize];
        for module in &modules {
            let offset = (module.base - base) as usize;
            mem[offset..offset + module.nso.image.len()].copy_from_slice(&module.nso.image);
        }
        let hle: HashSet<String> = hle.iter().cloned().collect();
        let mut exports = HashMap::new();
        for (index, module) in modules.iter().enumerate() {
            for (symbol, definition) in module.nso.symbols.iter().enumerate() {
                if definition.defined && !definition.name.is_empty() {
                    exports
                        .entry(definition.name.clone())
                        .or_insert((index, symbol));
                }
            }
        }
        let mut image = Self {
            base,
            end,
            mem,
            modules,
            hle,
            exports,
            hooks: BTreeMap::new(),
            hook_syms: HashMap::new(),
            tramp: HashMap::new(),
            unresolved: BTreeMap::new(),
            entries: HashSet::new(),
            init_arrays: BTreeMap::new(),
            eh_starts: Vec::new(),
            names: BTreeMap::new(),
            skipped,
            code: Vec::new(),
        };
        for symbol in image.hle.clone() {
            let Some((module, definition)) = image.exports.get(&symbol).copied() else {
                continue;
            };
            let name = c_ident(&symbol);
            let address =
                image.modules[module].base + image.modules[module].nso.symbols[definition].value;
            image.hooks.insert(address, name.clone());
            image.hook_syms.insert(name, symbol);
        }
        image.relocate()?;
        image.code = image
            .modules
            .iter()
            .map(|module| {
                let range = module
                    .nso
                    .segments
                    .first()
                    .map(|segment| (segment.offset, segment.size))
                    .unwrap_or((0, 0));
                (
                    module.base + range.0,
                    module.base + range.0 + range.1,
                    module.nso.name.clone(),
                )
            })
            .collect();
        image.find_entries();
        image.collect_names();
        Ok(image)
    }

    fn trampoline(&mut self, symbol: &str) -> u64 {
        if let Some(address) = self.tramp.get(symbol) {
            return *address;
        }
        let address = TRAMP_BASE + 16 * self.tramp.len() as u64;
        let name = c_ident(symbol);
        self.hooks.insert(address, name.clone());
        self.hook_syms.insert(name, symbol.to_owned());
        self.tramp.insert(symbol.to_owned(), address);
        address
    }

    fn resolve(&mut self, symbol: &str, weak: bool) -> u64 {
        if let Some((module, definition)) = self.exports.get(symbol).copied() {
            return self.modules[module].base + self.modules[module].nso.symbols[definition].value;
        }
        if weak && !self.hle.contains(symbol) {
            return 0;
        }
        if !self.hle.contains(symbol) {
            *self.unresolved.entry(symbol.to_owned()).or_default() += 1;
        }
        self.trampoline(symbol)
    }

    fn relocate(&mut self) -> Result<(), String> {
        let mut work = Vec::new();
        for module in &self.modules {
            let bind: HashMap<String, String> = module
                .nso
                .symbols
                .iter()
                .map(|symbol| (symbol.name.clone(), symbol.bind.clone()))
                .collect();
            work.push((
                module.nso.name.clone(),
                module.base,
                bind,
                module.nso.relocs.clone(),
            ));
        }
        for (name, base, bind, relocs) in work {
            for reloc in relocs {
                let at = (base + reloc.offset)
                    .checked_sub(self.base)
                    .ok_or("relocation before the image")? as usize;
                let value = match reloc.kind.as_str() {
                    "RELATIVE" => base.wrapping_add(reloc.addend as u64),
                    "ABS64" | "GLOB_DAT" | "JUMP_SLOT" => {
                        let symbol = reloc.sym.as_deref().unwrap_or("");
                        let weak = bind.get(symbol).map(String::as_str) == Some("weak");
                        self.resolve(symbol, weak).wrapping_add(reloc.addend as u64)
                    }
                    other => {
                        return Err(format!(
                            "{name}: relocation type {other} at {:#x} not supported",
                            reloc.offset
                        ));
                    }
                };
                let bytes = self
                    .mem
                    .get_mut(at..at + 8)
                    .ok_or("relocation outside the image")?;
                bytes.copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(())
    }

    fn in_code(&self, address: u64) -> bool {
        address & 3 == 0
            && self
                .code
                .iter()
                .any(|(lo, hi, _)| *lo <= address && address < *hi)
    }

    fn read_u64(&self, address: u64) -> Option<u64> {
        let at = address.checked_sub(self.base)? as usize;
        self.mem
            .get(at..at + 8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
    }

    #[allow(dead_code)]
    fn read_u32(&self, address: u64) -> Option<u32> {
        let at = address.checked_sub(self.base)? as usize;
        self.mem
            .get(at..at + 4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn find_entries(&mut self) {
        let mut entries = HashSet::new();
        for module in &self.modules {
            let range = module
                .nso
                .segments
                .first()
                .map(|segment| (segment.offset, segment.size))
                .unwrap_or((0, 0));
            let lo = module.base + range.0;
            let hi = lo + range.1;
            entries.insert(lo);
            for key in ["INIT", "FINI"] {
                if let Some(value) = module
                    .nso
                    .dynamic
                    .get(key)
                    .and_then(|values| values.first())
                {
                    entries.insert(module.base + value);
                }
            }
            for function in &module.nso.eh_funcs {
                entries.insert(module.base + *function as u64);
            }
            for symbol in &module.nso.symbols {
                if symbol.defined && symbol.kind == "func" && symbol.value != 0 {
                    entries.insert(module.base + symbol.value);
                }
            }
            for reloc in &module.nso.relocs {
                if matches!(reloc.kind.as_str(), "RELATIVE" | "ABS64") {
                    if let Some(value) = self.read_u64(module.base + reloc.offset) {
                        if self.in_code(value) {
                            entries.insert(value);
                        }
                    }
                }
            }
            let start = (lo - self.base) as usize;
            let end = (hi - self.base) as usize;
            for (i, word) in self.mem[start..end].chunks_exact(4).enumerate() {
                let word = u32::from_le_bytes(word.try_into().unwrap());
                if word & 0xFC00_0000 == 0x9400_0000 {
                    let off = word & 0x03FF_FFFF;
                    let signed = if off & (1 << 25) != 0 {
                        off as i64 - (1 << 26)
                    } else {
                        off as i64
                    };
                    entries.insert(lo + 4 * i as u64 + 4 * signed as u64);
                }
            }
        }
        let mut init_arrays = BTreeMap::new();
        for module in &self.modules {
            let start = module
                .nso
                .dynamic
                .get("INIT_ARRAY")
                .and_then(|values| values.first())
                .copied()
                .unwrap_or(0);
            let size = module
                .nso
                .dynamic
                .get("INIT_ARRAYSZ")
                .and_then(|values| values.first())
                .copied()
                .unwrap_or(0);
            let mut array = Vec::new();
            for i in 0..size / 8 {
                if let Some(address) = self.read_u64(module.base + start + 8 * i) {
                    array.push(address);
                }
            }
            for address in &array {
                if self.in_code(*address) {
                    entries.insert(*address);
                }
            }
            init_arrays.insert(module.nso.name.clone(), array);
        }
        self.entries = entries
            .into_iter()
            .filter(|address| self.in_code(*address))
            .collect();
        self.eh_starts = self
            .modules
            .iter()
            .flat_map(|module| {
                module
                    .nso
                    .eh_funcs
                    .iter()
                    .map(|function| module.base + *function as u64)
            })
            .collect();
        self.eh_starts.sort_unstable();
        self.init_arrays = init_arrays;
    }

    fn collect_names(&mut self) {
        let mut names = BTreeMap::new();
        for module in &self.modules {
            for symbol in &module.nso.symbols {
                if symbol.defined && symbol.kind == "func" && !symbol.name.is_empty() {
                    names
                        .entry(module.base + symbol.value)
                        .or_insert_with(|| symbol.name.clone());
                }
            }
        }
        self.names = names;
    }

    /// Function [start, end) spans from every module's .eh_frame_hdr.
    pub fn fde_spans(&self) -> Vec<(u64, u64)> {
        let mut spans = Vec::new();
        for module in &self.modules {
            let (start, end) = module.nso.eh_frame_hdr;
            if end <= start {
                continue;
            }
            let image = &module.nso.image;
            let Some(head) = image.get(start..start + 4) else {
                continue;
            };
            if head[0] != 1 || head[3] != 0x3B || head[2] != 0x03 {
                continue;
            }
            let Some(count) = image
                .get(start + 8..start + 12)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            else {
                continue;
            };
            for i in 0..count as usize {
                let at = start + 12 + 8 * i;
                let (Some(loc), Some(fde)) = (
                    image
                        .get(at..at + 4)
                        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap())),
                    image
                        .get(at + 4..at + 8)
                        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap())),
                ) else {
                    continue;
                };
                let loc = start as i64 + loc as i64;
                let fde = start as i64 + fde as i64;
                if fde < 0 || fde + 16 > image.len() as i64 {
                    continue;
                }
                let at = fde as usize;
                let Some(length) = image
                    .get(at..at + 4)
                    .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                else {
                    continue;
                };
                let Some(cie) = image
                    .get(at + 4..at + 8)
                    .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                else {
                    continue;
                };
                let Some(begin) = image
                    .get(at + 8..at + 12)
                    .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
                else {
                    continue;
                };
                let Some(range) = image
                    .get(at + 12..at + 16)
                    .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                else {
                    continue;
                };
                if length < 12 || cie == 0 || fde + 8 + begin as i64 != loc {
                    continue;
                }
                spans.push((
                    module.base + loc as u64,
                    module.base + loc as u64 + range as u64,
                ));
            }
        }
        spans.sort();
        spans
    }

    /// Entries at code addresses the code itself builds with ADR or
    /// ADRP+ADD, except targets strictly inside an unwind-covered function.
    pub fn add_adr_entries(&mut self, max_ahead: usize) -> (usize, usize, usize) {
        let spans = self.fde_spans();
        let mut targets = HashSet::new();
        for module in &self.modules {
            let Some(text) = module.nso.segments.first() else {
                continue;
            };
            let start = text.offset as usize;
            let end = (text.offset + text.size) as usize;
            let Some(bytes) = module.nso.image.get(start..end) else {
                continue;
            };
            let words: Vec<u32> = bytes
                .chunks_exact(4)
                .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
                .collect();
            for (i, word) in words.iter().copied().enumerate() {
                let pc = module.base + (start + 4 * i) as u64;
                if word & 0x9F00_0000 == 0x1000_0000 {
                    let imm = (((word >> 5) & 0x7FFFF) << 2) | ((word >> 29) & 3);
                    let imm = if imm & (1 << 20) != 0 {
                        imm as i64 - (1 << 21)
                    } else {
                        imm as i64
                    };
                    targets.insert(pc.wrapping_add(imm as u64));
                } else if word & 0x9F00_0000 == 0x9000_0000 {
                    let imm = (((word >> 5) & 0x7FFFF) << 2) | ((word >> 29) & 3);
                    let imm = if imm & (1 << 20) != 0 {
                        imm as i64 - (1 << 21)
                    } else {
                        imm as i64
                    };
                    let page = (pc & !0xFFF).wrapping_add((imm << 12) as u64);
                    for ahead in 1..=max_ahead {
                        let Some(other) = words.get(i + ahead).copied() else {
                            break;
                        };
                        let add =
                            other & 0xFFC0_0000 == 0x9100_0000 && (other >> 5) & 31 == word & 31;
                        if add {
                            targets.insert(page.wrapping_add(((other >> 10) & 0xFFF) as u64));
                        }
                    }
                }
            }
        }
        let text_ranges: Vec<(u64, u64)> = self.code.iter().map(|(lo, hi, _)| (*lo, *hi)).collect();
        let starts: HashSet<u64> = spans.iter().map(|(start, _)| *start).collect();
        let mut refs: Vec<u64> = targets
            .into_iter()
            .filter(|target| {
                target & 3 == 0
                    && text_ranges
                        .iter()
                        .any(|(lo, hi)| *lo <= *target && *target < *hi)
            })
            .collect();
        refs.sort_unstable();
        refs.dedup();
        let before = self.entries.len();
        for target in refs.iter().copied() {
            let inside = spans
                .iter()
                .any(|(start, end)| *start <= target && target < *end)
                && !starts.contains(&target);
            if !inside && self.in_code(target) {
                self.entries.insert(target);
            }
        }
        (refs.len(), self.entries.len() - before, spans.len())
    }

    pub fn json(&self) -> Value {
        json!({
            "base": self.base,
            "end": self.end,
            "tramp_base": TRAMP_BASE,
            "modules": self.modules.iter().map(|module| {
                let init = module.nso.dynamic.get("INIT").and_then(|values| values.first()).map(|value| module.base + value).unwrap_or(0);
                let fini = module.nso.dynamic.get("FINI").and_then(|values| values.first()).map(|value| module.base + value).unwrap_or(0);
                json!({
                    "name": module.nso.name,
                    "base": module.base,
                    "size": module.nso.image.len(),
                    "build_id": module.nso.build_id,
                    "init": init,
                    "fini": fini,
                    "segments": module.nso.segments.iter().map(|segment| json!({"name": segment.name, "addr": module.base + segment.offset, "size": segment.size})).collect::<Vec<_>>(),
                    "bss": [module.base + module.nso.bss.0 as u64, module.base + module.nso.bss.1 as u64],
                })
            }).collect::<Vec<_>>(),
            "skipped": self.skipped,
            "hooks": self.hooks.iter().map(|(address, name)| json!({"addr": address, "c": name, "sym": self.hook_syms[name]})).collect::<Vec<_>>(),
            "unresolved": self.unresolved,
            "init_arrays": self.init_arrays,
            "exports": self.exports.iter().filter_map(|(name, (module, symbol))| {
                let symbol = &self.modules[*module].nso.symbols[*symbol];
                (symbol.kind == "func").then(|| (name.clone(), json!(self.modules[*module].base + symbol.value)))
            }).collect::<HashMap<_, _>>(),
        })
    }

    pub fn write(&self, out: &Path) -> Result<Value, String> {
        fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
        fs::write(out.join("image.bin"), &self.mem)
            .map_err(|e| format!("{}: {e}", out.join("image.bin").display()))?;
        let meta = self.json();
        fs::write(
            out.join("image.json"),
            serde_json::to_vec(&meta).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{}: {e}", out.join("image.json").display()))?;
        Ok(meta)
    }

    #[allow(dead_code)]
    pub fn module_addresses(&self) -> Vec<(String, u64, usize)> {
        self.modules
            .iter()
            .map(|module| (module.nso.name.clone(), module.base, module.nso.image.len()))
            .collect()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)]
    pub fn names(&self) -> &BTreeMap<u64, String> {
        &self.names
    }
}

pub fn adr_json(exefs: &str, aslr: u64, hle: &[String], skip: &[String]) -> Result<Value, String> {
    let mut image = Image::build(Path::new(exefs), aslr, false, hle, skip)?;
    let (refs, new, spans) = image.add_adr_entries(4);
    Ok(json!({"refs": refs, "new": new, "spans": spans, "entries": image.entries.len()}))
}

pub fn build_json(
    exefs: &str,
    out: &str,
    aslr: u64,
    program_args: bool,
    hle: &[String],
    skip: &[String],
) -> Result<Value, String> {
    let image = Image::build(Path::new(exefs), aslr, program_args, hle, skip)?;
    let meta = image.write(Path::new(out))?;
    Ok(json!({
        "image": meta,
        "entries": image.entry_count(),
        "trampolines": image.tramp.len(),
        "unresolved": image.unresolved,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn identifiers_are_c_names() {
        assert_eq!(super::c_ident("nn::os::Sleep"), "imp_nn__os__Sleep");
        assert_eq!(super::c_ident("plain_name"), "imp_plain_name");
    }
}
