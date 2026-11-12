//! Function discovery over the guest image: walk from every entry until a
//! terminator or another entry. Ported from a64_lift.Lifter.walk; jump-table
//! resolution and C emission follow in later steps.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};

use crate::a64::{self, Kind};
use crate::image::Image;

const COND: [&str; 16] = [
    "(c->z)",
    "(!c->z)",
    "(c->c)",
    "(!c->c)",
    "(c->n)",
    "(!c->n)",
    "(c->v)",
    "(!c->v)",
    "(c->c && !c->z)",
    "(!(c->c && !c->z))",
    "(c->n == c->v)",
    "(c->n != c->v)",
    "((c->n == c->v) && !c->z)",
    "(!((c->n == c->v) && !c->z))",
    "(1)",
    "(1)",
];

pub trait Code {
    fn in_code(&self, address: u64) -> bool;
    fn word(&self, address: u64) -> Option<u32>;
    fn read(&self, address: u64, size: usize) -> Option<&[u8]>;
}

impl Code for Image {
    fn in_code(&self, address: u64) -> bool {
        Image::in_code(self, address)
    }
    fn word(&self, address: u64) -> Option<u32> {
        Image::word(self, address)
    }
    fn read(&self, address: u64, size: usize) -> Option<&[u8]> {
        Image::read(self, address, size)
    }
}

pub struct Lifter<'a, C: Code> {
    code: &'a C,
    pub entries: HashSet<u64>,
    pub eh_starts: Vec<u64>,
    pub funcs: BTreeMap<u64, BTreeMap<u64, Kind>>,
    pub labels: BTreeMap<u64, HashSet<u64>>,
    pub jt: BTreeMap<u64, Vec<u64>>,
    pub jt_missing: BTreeMap<u64, u8>,
    pub stats: BTreeMap<&'static str, usize>,
}

impl<'a, C: Code> Lifter<'a, C> {
    pub fn new(code: &'a C, entries: impl IntoIterator<Item = u64>) -> Self {
        let entries: HashSet<u64> = entries.into_iter().collect();
        let mut eh_starts: Vec<u64> = entries.iter().copied().collect();
        eh_starts.sort_unstable();
        Self {
            code,
            entries,
            eh_starts,
            funcs: BTreeMap::new(),
            labels: BTreeMap::new(),
            jt: BTreeMap::new(),
            jt_missing: BTreeMap::new(),
            stats: BTreeMap::new(),
        }
    }

    pub fn discover(&mut self) {
        let entries: Vec<u64> = self.entries.iter().copied().collect();
        for entry in entries {
            let (body, labels) = self.walk(entry);
            if !body.is_empty() {
                self.funcs.insert(entry, body);
                self.labels.insert(entry, labels);
            }
        }
        // Resolve the direct jump tables the bodies ended at, then walk the
        // functions their targets made reachable.
        let mut branches = Vec::new();
        for body in self.funcs.values() {
            for (pc, kind) in body {
                if let Kind::Br(reg) = kind {
                    branches.push((*pc, *reg));
                }
            }
        }
        let mut added = Vec::new();
        for (pc, reg) in branches {
            if let Some(targets) = self.jump_table(pc, reg) {
                self.jt.insert(pc, targets.clone());
                for target in targets {
                    if !self.funcs.contains_key(&target) && self.code.in_code(target) {
                        added.push(target);
                    }
                }
            }
        }
        for entry in added {
            let (body, labels) = self.walk(entry);
            if !body.is_empty() {
                self.funcs.insert(entry, body);
                self.labels.insert(entry, labels);
            }
        }
        self.stats.insert("jump_tables", self.jt.len());
        self.stats.insert("jt_missing", self.jt_missing.len());
    }

    fn function_extent(&self, pc: u64) -> (u64, u64) {
        let at = self.eh_starts.partition_point(|start| *start <= pc);
        let lo = if at > 0 { self.eh_starts[at - 1] } else { 0 };
        let hi = self.eh_starts.get(at).copied().unwrap_or(u64::MAX);
        (lo, hi)
    }

    fn writes(word: u32, reg: u8) -> bool {
        if (word >> 25) & 7 == 7 {
            return word & 31 == reg as u32 && matches!(word & 0x5F20FC00, 0x1E200000 | 0x0E002C00);
        }
        if word & 0x0A000000 == 0x08000000 && (word >> 26) & 1 != 0 {
            return false;
        }
        if word & 31 == reg as u32 && !Self::reads_rt_only(word) {
            return true;
        }
        (word >> 27) & 7 == 5
            && (word >> 22) & 1 != 0
            && (word >> 26) & 1 == 0
            && (word >> 10) & 31 == reg as u32
    }

    fn reads_rt_only(word: u32) -> bool {
        (word & 0x3A000000 == 0x38000000 && (word >> 22) & 1 == 0)
            || word & 0x1C000000 == 0x14000000
            || matches!(word & 0x7E000000, 0x34000000 | 0x36000000)
            || ((word >> 27) & 7 == 5 && (word >> 22) & 1 == 0)
    }

    fn adr_targets(&self, br_pc: u64, reg: u8) -> Option<Vec<u64>> {
        let (lo, hi) = self.function_extent(br_pc);
        if hi.saturating_sub(lo) > 0x40000 {
            return None;
        }
        let mut targets = Vec::new();
        let mut pc = lo;
        while pc < hi {
            if let Some(word) = self.code.word(pc) {
                if word & 0x9F00001F == 0x10000000 | reg as u32 {
                    let imm = ((word >> 3) & 0x1FFFFC) | ((word >> 29) & 3);
                    let target = pc.wrapping_add(a64::sx(imm, 21) as u64);
                    if lo <= target && target < hi && self.code.in_code(target) {
                        targets.push(target);
                    }
                }
            }
            pc += 4;
        }
        targets.sort_unstable();
        targets.dedup();
        if targets.is_empty() {
            None
        } else {
            Some(targets)
        }
    }

    fn hoisted_constant(&self, before: u64, reg: u8, scan: usize) -> Option<u64> {
        let mut pc = before;
        for _ in 0..scan {
            pc = pc.checked_sub(4)?;
            let word = self.code.word(pc)?;
            if word & 31 != reg as u32 || Self::reads_rt_only(word) {
                continue;
            }
            if word & 0x9F000000 == 0x90000000 {
                let imm = ((word >> 3) & 0x1FFFFC) | ((word >> 29) & 3);
                return Some((pc & !0xFFF).wrapping_add((a64::sx(imm, 21) << 12) as u64));
            }
            if word & 0xFF800000 == 0x91000000 {
                let base = self.hoisted_constant(pc, ((word >> 5) & 31) as u8, 64)?;
                let imm = ((word >> 10) & 0xFFF) << if word & (1 << 22) != 0 { 12 } else { 0 };
                return Some(base.wrapping_add(imm as u64));
            }
            return None;
        }
        None
    }

    /// A direct bounded jump table: recognise `adrp/add` table setups and
    /// `ldr/ldrsw` entries in the window before the BR. Table addresses that
    /// are only known through the CFG are left to jt_missing.
    pub fn jump_table(&mut self, br_pc: u64, reg: u8) -> Option<Vec<u64>> {
        #[derive(Clone)]
        enum V {
            Const(u64),
            Reg(u8),
            Load {
                table: Box<V>,
                index: u8,
                size: u8,
                signed: bool,
            },
            Add {
                left: Box<V>,
                right: Box<V>,
                shift: u8,
            },
        }
        let mut values: std::collections::HashMap<u8, V> = std::collections::HashMap::new();
        let mut bounds: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
        let mut cmp: Option<(u8, usize)> = None;
        for pc in (br_pc.saturating_sub(128)..br_pc).step_by(4) {
            let Some(word) = self.code.word(pc) else {
                break;
            };
            let d = (word & 31) as u8;
            let n = ((word >> 5) & 31) as u8;
            let m = ((word >> 16) & 31) as u8;
            let value = match word {
                w if w & 0x9F000000 == 0x90000000 => {
                    let imm = ((w >> 3) & 0x1FFFFC) | ((w >> 29) & 3);
                    Some(V::Const(
                        (pc & !0xFFF).wrapping_add((a64::sx(imm, 21) << 12) as u64),
                    ))
                }
                w if w & 0x9F000000 == 0x10000000 => {
                    let imm = ((w >> 3) & 0x1FFFFC) | ((w >> 29) & 3);
                    Some(V::Const(pc.wrapping_add(a64::sx(imm, 21) as u64)))
                }
                w if w & 0xFF800000 == 0x91000000 => {
                    let v = values.get(&n).cloned().unwrap_or(V::Reg(n));
                    let imm =
                        (((w >> 10) & 0xFFF) << if w & (1 << 22) != 0 { 12 } else { 0 }) as u64;
                    match v {
                        V::Const(base) => Some(V::Const(base.wrapping_add(imm))),
                        V::Add { .. } => None,
                        _ => Some(V::Add {
                            left: Box::new(v),
                            right: Box::new(V::Const(imm)),
                            shift: 0,
                        }),
                    }
                }
                w if w & 0x3B200C00 == 0x38200800 && (w >> 26) & 1 == 0 => {
                    let size = (w >> 30) as u8;
                    let opc = (w >> 22) & 3;
                    let base = values.get(&n).cloned().unwrap_or(V::Reg(n));
                    if opc != 0 {
                        Some(V::Load {
                            table: Box::new(base),
                            index: m,
                            size,
                            signed: opc >= 2,
                        })
                    } else {
                        None
                    }
                }
                w if w & 0xFF200000 == 0x8B000000 && (w >> 22) & 3 == 0 => {
                    let left = values.get(&n).cloned().unwrap_or(V::Reg(n));
                    let right = values.get(&m).cloned().unwrap_or(V::Reg(m));
                    Some(V::Add {
                        left: Box::new(left),
                        right: Box::new(right),
                        shift: ((w >> 10) & 63) as u8,
                    })
                }
                w if w & 0x7F80001F == 0x7100001F => {
                    cmp = Some((n, ((w >> 10) & 0xFFF) as usize));
                    None
                }
                w if matches!(w & 0xFF00001F, 0x54000008 | 0x54000002) && cmp.is_some() => {
                    let (r, imm) = cmp.unwrap();
                    if w & 15 == 8 {
                        bounds.insert(r, imm + 1);
                    }
                    None
                }
                w if w & 0x7F800000 == 0x12000000 && (w >> 16) & 63 == 0 => {
                    let imms = (w >> 10) & 63;
                    if (w >> 22) & 1 != 0 || (w >> 31) == 0 {
                        if imms < 31 {
                            bounds.insert(d, 1 << (imms + 1));
                        }
                    }
                    None
                }
                w if w & 0x7FE0FFE0 == 0x2A0003E0 => {
                    if let Some(bound) = bounds.get(&m).copied() {
                        bounds.insert(d, bound);
                    }
                    None
                }
                w if d != 31 && Self::writes(w, d) => None,
                _ => values.get(&d).cloned(),
            };
            if let Some(value) = value {
                values.insert(d, value);
            } else {
                values.remove(&d);
            }
        }
        let Some(value) = values.get(&reg).cloned() else {
            return self.adr_targets(br_pc, reg);
        };
        let (base, load) = match value {
            V::Add { left, right, .. } => match (*left, *right) {
                (V::Const(base), load @ V::Load { .. }) => (base, load),
                (load @ V::Load { .. }, V::Const(base)) => (base, load),
                _ => return self.adr_targets(br_pc, reg),
            },
            _ => return self.adr_targets(br_pc, reg),
        };
        let V::Load {
            table,
            index,
            size,
            signed,
        } = load
        else {
            return self.adr_targets(br_pc, reg);
        };
        let table = match *table {
            V::Const(table) => table,
            V::Reg(reg) => self.hoisted_constant(br_pc, reg, 1024)?,
            V::Add { left, right, .. } => match (*left, *right) {
                (V::Reg(reg), V::Const(offset)) => self
                    .hoisted_constant(br_pc, reg, 1024)?
                    .wrapping_add(offset),
                _ => return self.adr_targets(br_pc, reg),
            },
            _ => return self.adr_targets(br_pc, reg),
        };
        let Some(count) = bounds
            .get(&index)
            .copied()
            .filter(|count| *count > 0 && *count <= 4096)
        else {
            self.jt_missing.insert(br_pc, reg);
            self.stats.insert(
                "jt_missing_no_bound",
                self.stats.get("jt_missing_no_bound").unwrap_or(&0) + 1,
            );
            return None;
        };
        let (lo, hi) = self.function_extent(br_pc);
        let mut targets = Vec::new();
        for i in 0..count {
            let at = table + i as u64 * size as u64;
            let Some(bytes) = self.code.read(at, size as usize) else {
                break;
            };
            let entry = match (size, signed) {
                (1, true) => bytes[0] as i8 as i64,
                (1, false) => bytes[0] as i64,
                (2, true) => i16::from_le_bytes(bytes.try_into().unwrap()) as i64,
                (2, false) => u16::from_le_bytes(bytes.try_into().unwrap()) as i64,
                (4, true) => i32::from_le_bytes(bytes.try_into().unwrap()) as i64,
                (4, false) => u32::from_le_bytes(bytes.try_into().unwrap()) as i64,
                (8, true) => i64::from_le_bytes(bytes.try_into().unwrap()),
                (8, false) => u64::from_le_bytes(bytes.try_into().unwrap()) as i64,
                _ => break,
            };
            let target = base.wrapping_add(entry as u64);
            if !(lo <= target && target < hi && self.code.in_code(target)) {
                break;
            }
            targets.push(target);
        }
        if targets.is_empty() {
            self.jt_missing.insert(br_pc, reg);
            return None;
        }
        self.stats
            .insert("jt_bounded", self.stats.get("jt_bounded").unwrap_or(&0) + 1);
        Some(targets)
    }

    pub fn walk(&self, entry: u64) -> (BTreeMap<u64, Kind>, HashSet<u64>) {
        let mut body = BTreeMap::new();
        let mut labels = HashSet::new();
        let mut todo = vec![entry];
        while let Some(mut pc) = todo.pop() {
            while !body.contains_key(&pc) {
                if (pc != entry && self.entries.contains(&pc)) || !self.code.in_code(pc) {
                    break;
                }
                let Some(word) = self.code.word(pc) else {
                    break;
                };
                let kind = a64::classify(word, pc);
                body.insert(pc, kind);
                let mut visit = |target: u64| {
                    if target != entry && self.entries.contains(&target) {
                        return;
                    }
                    if self.code.in_code(target) {
                        labels.insert(target);
                        todo.push(target);
                    }
                };
                match kind {
                    Kind::B(target) => {
                        visit(target);
                        break;
                    }
                    Kind::BCond(target, _)
                    | Kind::Cbz(target, _, _, _)
                    | Kind::Tbz(target, _, _, _) => {
                        visit(target);
                        pc = pc.wrapping_add(4);
                    }
                    Kind::Br(_) => break,
                    Kind::Ret(_) | Kind::Trap => break,
                    _ => pc = pc.wrapping_add(4),
                }
            }
        }
        (body, labels)
    }

    /// C for one discovered function. This is the plain control-flow emission;
    /// the flag, frame and local-register transforms follow separately.
    pub fn emit_function(&self, entry: u64, texts: &[String]) -> String {
        let Some(body) = self.funcs.get(&entry) else {
            return String::new();
        };
        let labels = &self.labels[&entry];
        let mut out = vec![format!(
            "void f_{entry:08x}(Ctx *__restrict A64_NOESC c) {{"
        )];
        let pcs: Vec<u64> = body.keys().copied().collect();
        for (index, pc) in pcs.iter().copied().enumerate() {
            let kind = body[&pc];
            let word = self.code.word(pc).unwrap_or(0);
            if labels.contains(&pc) || (pc == entry && labels.contains(&entry)) {
                out.push(format!("L_{pc:08x}:;"));
            }
            let mut falls = true;
            match kind {
                Kind::Op => {
                    let text = texts.get(index).map(String::as_str).unwrap_or("");
                    if text.is_empty() {
                        out.push(format!("a64_trap(c, 0x{pc:x}ULL, 0x{word:08x}u); return;"));
                        falls = false;
                    } else {
                        out.push(text.trim_end_matches('\n').to_owned());
                    }
                }
                Kind::B(target) => {
                    out.push(self.transfer(pc, target, body));
                    falls = false;
                }
                Kind::BCond(target, cond) => {
                    out.push(format!(
                        "if {} {{ {} }}",
                        COND[cond as usize],
                        self.transfer(pc, target, body)
                    ));
                }
                Kind::Cbz(target, reg, sf, nz) => {
                    let value = if reg == 31 {
                        "0".to_owned()
                    } else if sf != 0 {
                        format!("c->x[{reg}]")
                    } else {
                        format!("(uint32_t)c->x[{reg}]")
                    };
                    out.push(format!(
                        "if ({value} {} 0) {{ {} }}",
                        if nz != 0 { "!=" } else { "==" },
                        self.transfer(pc, target, body)
                    ));
                }
                Kind::Tbz(target, reg, bit, nz) => {
                    let value = if reg == 31 {
                        "0".to_owned()
                    } else {
                        format!("((c->x[{reg}] >> {bit}) & 1)")
                    };
                    out.push(format!(
                        "if {}{value} {{ {} }}",
                        if nz != 0 { "" } else { "!" },
                        self.transfer(pc, target, body)
                    ));
                }
                Kind::Bl(target) => {
                    let next = pc + 4;
                    if self.entries.contains(&target) {
                        out.push(format!("c->x[30] = 0x{next:x}ULL; f_{target:08x}(c);"));
                    } else {
                        out.push(format!("a64_trap(c, 0x{pc:x}ULL, 0x{word:08x}u); return;"));
                        falls = false;
                    }
                }
                Kind::Blr(reg) => {
                    out.push(format!(
                        "{{ uint64_t _t = c->x[{reg}]; c->x[30] = 0x{:x}ULL; A64_ICALL(c, _t); }}",
                        pc + 4
                    ));
                }
                Kind::Ret(_) => {
                    out.push("return;".to_owned());
                    falls = false;
                }
                Kind::Br(reg) => {
                    out.push(format!("A64_IJUMP(c, c->x[{reg}]); return;"));
                    falls = false;
                }
                Kind::Svc(imm) => out.push(format!("a64_svc(c, {imm}u);")),
                Kind::Trap => {
                    out.push(format!("a64_trap(c, 0x{pc:x}ULL, 0x{word:08x}u); return;"));
                    falls = false;
                }
                Kind::Isb => out.push("recomp_barrier();".to_owned()),
                Kind::Ic => out.push("/* ic ivau: no self-modifying code */".to_owned()),
            }
            if falls && (index + 1 == pcs.len() || pcs[index + 1] != pc + 4) {
                out.push(self.transfer(pc, pc + 4, body));
            }
        }
        out.push("}".to_owned());
        out.join("\n") + "\n"
    }

    fn transfer(&self, pc: u64, target: u64, body: &BTreeMap<u64, Kind>) -> String {
        if body.contains_key(&target) && (!self.entries.contains(&target) || target == pc) {
            format!("goto L_{target:08x};")
        } else if self.entries.contains(&target) {
            format!("TAILCALL(f_{target:08x});")
        } else {
            format!("a64_trap(c, 0x{target:x}ULL, 0); return;")
        }
    }

    pub fn json(&self) -> serde_json::Value {
        let instructions: usize = self.funcs.values().map(|body| body.len()).sum();
        serde_json::json!({
            "functions": self.funcs.len(),
            "instructions": instructions,
            "labels": self.labels.values().map(|set| set.len()).sum::<usize>(),
            "entries": self.entries.len(),
        })
    }
}

pub fn tables_json(exefs: &str) -> Result<serde_json::Value, String> {
    let image = crate::image::Image::build(std::path::Path::new(exefs), 0, false, &[], &[])?;
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.eh_starts = image.eh_starts().to_vec();
    lift.discover();
    Ok(serde_json::json!({
        "functions": lift.funcs.len(),
        "jump_tables": lift.jt.len(),
        "jt_missing": lift.jt_missing.len(),
        "targets": lift.jt.values().map(Vec::len).sum::<usize>(),
        "stats": lift.stats,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mock(Vec<u32>);
    impl Code for Mock {
        fn in_code(&self, address: u64) -> bool {
            (address as usize) < self.0.len() * 4
        }
        fn word(&self, address: u64) -> Option<u32> {
            self.0.get((address / 4) as usize).copied()
        }
        fn read(&self, address: u64, size: usize) -> Option<&[u8]> {
            let at = address as usize;
            let end = at + size;
            if end > self.0.len() * 4 {
                return None;
            }
            let bytes =
                unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast::<u8>().add(at), size) };
            Some(bytes)
        }
    }

    #[test]
    fn emits_a_simple_function() {
        let words = vec![0xD503_201F, 0x9100_0000, 0xD65F_03C0];
        let mock = Mock(words);
        let mut lifter = Lifter::new(&mock, [0]);
        lifter.discover();
        let items: Vec<(u64, u32)> = lifter.funcs[&0]
            .iter()
            .map(|(pc, _)| (*pc, mock.word(*pc).unwrap()))
            .collect();
        let Ok((texts, _)) = crate::emitter::translate(&items) else {
            return;
        };
        let text = lifter.emit_function(0, &texts);
        assert!(text.starts_with("void f_00000000(Ctx"));
        assert!(text.contains("/* nop/hint */"));
        assert!(text.contains("return;"));
        assert!(text.ends_with("}\n"));
    }

    #[test]
    fn walks_through_a_conditional_to_the_return() {
        let words = vec![
            0xD503_201F, // nop
            0x54FF_FFE0, // b.eq -4 (back to nop)
            0xD65F_03C0, // ret
            0x9100_0000, // add (reachable only by fallthrough)
        ];
        let mock = Mock(words);
        let mut lifter = Lifter::new(&mock, [0]);
        lifter.discover();
        let body = &lifter.funcs[&0];
        assert!(body.contains_key(&0));
        assert!(body.contains_key(&4));
        assert!(body.contains_key(&8));
        assert!(!body.contains_key(&12));
        assert!(lifter.labels[&0].contains(&0));
    }
}
