//! Function discovery over the guest image: walk from every entry until a
//! terminator or another entry. Ported from a64_lift.Lifter.walk; jump-table
//! resolution and C emission follow in later steps.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};

use sha1::{Digest, Sha1};

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

#[derive(Clone, Copy)]
enum Loc {
    Reg(u8),
    Slot(usize),
}

pub struct Lifter<'a, C: Code> {
    code: &'a C,
    pub entries: HashSet<u64>,
    pub eh_starts: Vec<u64>,
    pub funcs: BTreeMap<u64, BTreeMap<u64, Kind>>,
    pub labels: BTreeMap<u64, HashSet<u64>>,
    pub jt: BTreeMap<u64, Vec<u64>>,
    pub jt_missing: BTreeMap<u64, u8>,
    pub plt: BTreeMap<u64, (u64, u64)>,
    pub last_table_debug: Option<String>,
    pub names: BTreeMap<u64, String>,
    pub hooks: BTreeMap<u64, String>,
    /// C appended after the plain instruction at a guest address
    /// (nx2/oot_wide.py, nx2/oot_jitter.py, nx2/oot_occlusion.py).
    pub after: BTreeMap<u64, String>,
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
            plt: BTreeMap::new(),
            last_table_debug: None,
            names: BTreeMap::new(),
            hooks: BTreeMap::new(),
            after: BTreeMap::new(),
            stats: BTreeMap::new(),
        }
    }

    pub fn discover(&mut self) {
        let mut entries: Vec<u64> = self.entries.iter().copied().collect();
        entries.sort_unstable();
        for entry in entries {
            if self.hooks.contains_key(&entry) {
                continue;
            }
            let (body, labels) = self.walk(entry);
            if !body.is_empty() {
                self.funcs.insert(entry, body);
                self.labels.insert(entry, labels);
            }
        }
        // Resolve tables, then re-walk the functions that contain them so
        // their targets become labels in the same body (not new entries).
        for _ in 0..16 {
            let mut branches = Vec::new();
            for (entry, body) in &self.funcs {
                for (pc, kind) in body {
                    if let Kind::Br(reg) = kind {
                        if !self.jt.contains_key(pc) && !self.jt_missing.contains_key(pc) {
                            branches.push((*entry, *pc, *reg));
                        }
                    }
                }
            }
            let mut redo = Vec::new();
            for (entry, pc, reg) in branches {
                if let Some(targets) = self.jump_table(pc, reg) {
                    self.jt.insert(pc, targets);
                    redo.push(entry);
                }
            }
            if !redo.is_empty() {
                redo.sort_unstable();
                redo.dedup();
                for entry in redo {
                    let (body, labels) = self.walk(entry);
                    if !body.is_empty() {
                        self.funcs.insert(entry, body);
                        self.labels.insert(entry, labels);
                    }
                }
                continue;
            }
            let missing: Vec<(u64, u8)> = self
                .jt_missing
                .iter()
                .map(|(pc, reg)| (*pc, *reg))
                .collect();
            let mut resolved = false;
            for (pc, reg) in missing {
                if let Some(targets) = self.jump_table(pc, reg) {
                    self.jt.insert(pc, targets);
                    self.jt_missing.remove(&pc);
                    resolved = true;
                }
            }
            if !resolved {
                break;
            }
        }
        let entries: Vec<u64> = self.funcs.keys().copied().collect();
        for entry in entries {
            if let Some(target) = self.plt_target(entry) {
                self.plt.insert(entry, target);
            }
        }
        // A function calling __setjmp gets its return addresses walked too.
        let mut returns = Vec::new();
        for body in self.funcs.values() {
            for (pc, kind) in body {
                if let Kind::Bl(target) = kind {
                    let callee = self.callee(*target);
                    if self
                        .names
                        .get(&callee)
                        .map(|name| matches!(name.as_str(), "__setjmp" | "setjmp" | "_setjmp"))
                        .unwrap_or(false)
                    {
                        returns.push(pc + 4);
                    }
                }
            }
        }
        returns.sort_unstable();
        returns.dedup();
        for entry in returns {
            if !self.funcs.contains_key(&entry) && self.code.in_code(entry) {
                let (body, labels) = self.walk(entry);
                if !body.is_empty() {
                    self.funcs.insert(entry, body);
                    self.labels.insert(entry, labels);
                }
            }
        }
        self.stats.insert("jump_tables", self.jt.len());
        self.stats.insert("jt_missing", self.jt_missing.len());
        self.stats.insert("plt_stubs", self.plt.len());
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

    fn callee(&self, mut target: u64) -> u64 {
        for _ in 0..4 {
            let Some((next, _)) = self.plt.get(&target) else {
                break;
            };
            target = *next;
        }
        target
    }

    fn plt_target(&self, entry: u64) -> Option<(u64, u64)> {
        let mut words = [0u32; 4];
        for (i, word) in words.iter_mut().enumerate() {
            *word = self.code.word(entry + 4 * i as u64)?;
        }
        if words[0] & 0x9F00001F != 0x90000010
            || words[1] & 0xFFC003FF != 0xF9400211
            || words[2] & 0xFFC003FF != 0x91000210
            || words[3] != 0xD61F0220
        {
            return None;
        }
        let imm = ((words[0] >> 3) & 0x1FFFFC) | ((words[0] >> 29) & 3);
        let slot = (entry & !0xFFF)
            .wrapping_add((a64::sx(imm, 21) << 12) as u64)
            .wrapping_add(((words[1] >> 10) & 0xFFF) as u64 * 8);
        let bytes = self.code.read(slot, 8)?;
        let target = u64::from_le_bytes(bytes.try_into().ok()?);
        self.entries.contains(&target).then_some((target, slot))
    }

    fn predecessors(&self, lo: u64, hi: u64) -> std::collections::HashMap<u64, Vec<u64>> {
        let mut map: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
        let mut pc = lo;
        while pc < hi {
            let Some(word) = self.code.word(pc) else {
                break;
            };
            let kind = a64::classify(word, pc);
            let mut successors = Vec::new();
            match kind {
                Kind::B(target) => successors.push(target),
                Kind::BCond(target, _)
                | Kind::Cbz(target, _, _, _)
                | Kind::Tbz(target, _, _, _) => {
                    successors.push(target);
                    successors.push(pc + 4);
                }
                Kind::Br(_) => successors.extend(self.jt.get(&pc).into_iter().flatten().copied()),
                Kind::Ret(_) | Kind::Trap => {}
                _ => successors.push(pc + 4),
            }
            for successor in successors {
                map.entry(successor).or_default().push(pc);
            }
            pc += 4;
        }
        map
    }

    fn reaching(&self, at: u64, location: Loc, limit: usize, depth: u8) -> HashSet<u64> {
        let (lo, hi) = self.function_extent(at);
        if hi.saturating_sub(lo) > 0x40000 || depth > 4 {
            return HashSet::new();
        }
        let preds = self.predecessors(lo, hi);
        let mut found = HashSet::new();
        let mut seen = HashSet::new();
        let mut todo = vec![at];
        while let Some(pc_at) = todo.pop() {
            if seen.len() >= limit {
                break;
            }
            for p in preds.get(&pc_at).into_iter().flatten().copied() {
                if !seen.insert(p) {
                    continue;
                }
                let Some(word) = self.code.word(p) else {
                    continue;
                };
                match location {
                    Loc::Reg(reg) => {
                        if matches!(a64::classify(word, p), Kind::Bl(_) | Kind::Blr(_))
                            && (reg <= 18 || reg == 30)
                        {
                            continue;
                        }
                        if !Self::writes(word, reg) {
                            todo.push(p);
                            continue;
                        }
                        if word & 0x9F000000 == 0x90000000 {
                            let imm = ((word >> 3) & 0x1FFFFC) | ((word >> 29) & 3);
                            found
                                .insert((p & !0xFFF).wrapping_add((a64::sx(imm, 21) << 12) as u64));
                        } else if word & 0xFF800000 == 0x91000000 {
                            if let Some(base) =
                                self.hoisted_constant(p, ((word >> 5) & 31) as u8, 64)
                            {
                                found.insert(base.wrapping_add(
                                    (((word >> 10) & 0xFFF)
                                        << if word & (1 << 22) != 0 { 12 } else { 0 })
                                        as u64,
                                ));
                            }
                        } else if word & 0xFFE0FFE0 == 0xAA0003E0 {
                            found.extend(self.reaching(
                                p,
                                Loc::Reg(((word >> 16) & 31) as u8),
                                limit,
                                depth + 1,
                            ));
                        } else if word & 0xFFC003E0 == 0xF94003E0 {
                            found.extend(self.reaching(
                                p,
                                Loc::Slot((((word >> 10) & 0xFFF) * 8) as usize),
                                limit,
                                depth + 1,
                            ));
                        } else if word & 0xFFC003E0 == 0xA94003E0 {
                            let offset = (a64::sx((word >> 15) & 0x7F, 7) * 8
                                + if (word & 31) as u8 != reg { 8 } else { 0 })
                                as usize;
                            found.extend(self.reaching(p, Loc::Slot(offset), limit, depth + 1));
                        }
                    }
                    Loc::Slot(slot) => {
                        if word & 0xFFC003E0 == 0xF90003E0
                            && (((word >> 10) & 0xFFF) * 8) as usize == slot
                        {
                            found.extend(self.reaching(
                                p,
                                Loc::Reg((word & 31) as u8),
                                limit,
                                depth + 1,
                            ));
                        } else if word & 0xFFC003E0 == 0xA90003E0 {
                            let offset = (a64::sx((word >> 15) & 0x7F, 7) * 8) as usize;
                            if slot == offset || slot == offset + 8 {
                                let reg = if slot == offset {
                                    (word & 31) as u8
                                } else {
                                    ((word >> 10) & 31) as u8
                                };
                                found.extend(self.reaching(p, Loc::Reg(reg), limit, depth + 1));
                            } else {
                                todo.push(p);
                            }
                        } else if ((word & 31) == 31
                            && matches!(word & 0x7F800000, 0x51000000 | 0x11000000))
                            || ((word >> 5) & 31 == 31
                                && matches!(word & 0x3B200C00, 0x38000C00 | 0x38000400))
                            || (word & 0x3A800000 == 0x28800000)
                        {
                            continue;
                        } else {
                            todo.push(p);
                        }
                    }
                }
            }
        }
        found
    }

    fn function_constants(&self, br_pc: u64, reg: u8) -> Vec<u64> {
        let (lo, hi) = self.function_extent(br_pc);
        if hi.saturating_sub(lo) > 0x40000 {
            return Vec::new();
        }
        let mut pages = std::collections::HashMap::new();
        let mut found = std::collections::HashSet::new();
        let mut pc = lo;
        while pc < hi {
            let Some(word) = self.code.word(pc) else {
                break;
            };
            let d = (word & 31) as u8;
            if word & 0x9F000000 == 0x90000000 {
                let imm = ((word >> 3) & 0x1FFFFC) | ((word >> 29) & 3);
                pages.insert(
                    d,
                    (pc & !0xFFF).wrapping_add((a64::sx(imm, 21) << 12) as u64),
                );
            } else if word & 0xFF800000 == 0x91000000 {
                if let Some(base) = pages.get(&(((word >> 5) & 31) as u8)).copied() {
                    let value = base.wrapping_add(
                        (((word >> 10) & 0xFFF) << if word & (1 << 22) != 0 { 12 } else { 0 })
                            as u64,
                    );
                    if d == reg {
                        found.insert(value);
                    }
                }
            } else if word & 0x9F000000 == 0x10000000 && d == reg {
                let imm = ((word >> 3) & 0x1FFFFC) | ((word >> 29) & 3);
                found.insert(pc.wrapping_add(a64::sx(imm, 21) as u64));
            }
            pc += 4;
        }
        let mut found: Vec<u64> = found.into_iter().collect();
        found.sort_unstable();
        found
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

    fn read_table_targets(
        &self,
        tables: &[u64],
        base: u64,
        shift: u8,
        count: usize,
        size: u8,
        signed: bool,
        lo: u64,
        hi: u64,
    ) -> Vec<u64> {
        let mut targets = Vec::new();
        for table in tables {
            for i in 0..count {
                let at = table.wrapping_add(i as u64 * size as u64);
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
                let target = base.wrapping_add((entry as u64).wrapping_shl(shift as u32));
                if !(lo <= target && target < hi && self.code.in_code(target)) {
                    break;
                }
                targets.push(target);
            }
        }
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// Where a hoisted table register is resolved: at the BR, unless the
    /// window writes the register again after the table load (a reused
    /// register, nx2/lift_fixes.py's table_load) -- then at the load, where
    /// the register still holds the table.
    fn table_lookup_pc(&self, load_pc: u64, br_pc: u64, reg: u8) -> u64 {
        let mut pc = load_pc.wrapping_add(4);
        while pc < br_pc {
            if let Some(word) = self.code.word(pc) {
                if Self::writes(word, reg) {
                    return load_pc;
                }
            }
            pc += 4;
        }
        br_pc
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
                pc: u64,
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
        let mut window = 32usize;
        for k in 1..=32 {
            let Some(pc) = br_pc.checked_sub(4 * k as u64) else {
                break;
            };
            let Some(word) = self.code.word(pc) else {
                break;
            };
            if word == 0xD65F_0BFF
                || word == 0xD65F_0FFF
                || word & 0xFFFF_F81F == 0xD61F_081F
                || word & 0xFFFF_F800 == 0xD71F_0800
            {
                window = k - 1;
                break;
            }
        }
        for pc in (br_pc.saturating_sub(4 * window as u64)..br_pc).step_by(4) {
            let Some(word) = self.code.word(pc) else {
                break;
            };
            if word & 0xFC000000 == 0x14000000
                || matches!(word & 0xFFFFFC1F, 0xD65F0000 | 0xD61F0000)
            {
                values.clear();
                bounds.clear();
                continue;
            }
            let d = (word & 31) as u8;
            let n = ((word >> 5) & 31) as u8;
            let m = ((word >> 16) & 31) as u8;
            let value = match word {
                w if w & 0x9F000000 == 0x10000000 => {
                    let imm = ((w >> 3) & 0x1FFFFC) | ((w >> 29) & 3);
                    Some(V::Const(pc.wrapping_add(a64::sx(imm, 21) as u64)))
                }
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
                        V::Reg(reg) if d == reg => None,
                        _ => Some(V::Add {
                            left: Box::new(v),
                            right: Box::new(V::Const(imm)),
                            shift: 0,
                        }),
                    }
                }
                w if w & 0x3B200C00 == 0x38200800 && (w >> 26) & 1 == 0 => {
                    let size = 1u8 << (w >> 30);
                    let opc = (w >> 22) & 3;
                    let base = values.get(&n).cloned().unwrap_or(V::Reg(n));
                    if opc != 0 {
                        Some(V::Load {
                            table: Box::new(base),
                            index: m,
                            size,
                            signed: opc >= 2,
                            pc,
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
            self.stats.insert(
                "jt_no_value",
                self.stats.get("jt_no_value").unwrap_or(&0) + 1,
            );
            return self.adr_targets(br_pc, reg);
        };
        let (base, load, shift) = match value {
            V::Add { left, right, shift } => match (*left, *right) {
                (V::Const(base), load @ V::Load { .. }) => (base, load, shift),
                (load @ V::Load { .. }, V::Const(base)) => (base, load, shift),
                _ => return self.adr_targets(br_pc, reg),
            },
            _ => {
                self.stats
                    .insert("jt_not_add", self.stats.get("jt_not_add").unwrap_or(&0) + 1);
                return self.adr_targets(br_pc, reg);
            }
        };
        let V::Load {
            table,
            index,
            size,
            signed,
            pc: load_pc,
        } = load
        else {
            self.stats.insert(
                "jt_not_load",
                self.stats.get("jt_not_load").unwrap_or(&0) + 1,
            );
            return self.adr_targets(br_pc, reg);
        };
        let mut at_load = false;
        let tables: Vec<u64> = match *table {
            V::Const(table) => vec![table],
            V::Reg(treg) => {
                let at = self.table_lookup_pc(load_pc, br_pc, treg);
                at_load = at != br_pc;
                let mut tables: Vec<u64> = self
                    .reaching(at, Loc::Reg(treg), 50000, 0)
                    .into_iter()
                    .collect();
                if tables.is_empty() {
                    tables.extend(self.function_constants(at, treg));
                }
                if tables.is_empty() {
                    if let Some(hoisted) = self.hoisted_constant(at, treg, 1024) {
                        tables.push(hoisted);
                    }
                }
                tables.sort_unstable();
                tables.dedup();
                tables
            }
            V::Add { left, right, .. } => match (*left, *right) {
                (V::Reg(treg), V::Const(offset)) => {
                    let at = self.table_lookup_pc(load_pc, br_pc, treg);
                    at_load = at != br_pc;
                    let mut tables: Vec<u64> = self
                        .reaching(at, Loc::Reg(treg), 50000, 0)
                        .into_iter()
                        .collect();
                    if tables.is_empty() {
                        tables.extend(self.function_constants(at, treg));
                    }
                    if tables.is_empty() {
                        if let Some(hoisted) = self.hoisted_constant(at, treg, 1024) {
                            tables.push(hoisted);
                        }
                    }
                    tables
                        .iter()
                        .map(|table| table.wrapping_add(offset))
                        .collect()
                }
                _ => return self.adr_targets(br_pc, reg),
            },
            _ => return self.adr_targets(br_pc, reg),
        };
        if tables.is_empty() {
            self.stats.insert(
                "jt_no_table",
                self.stats.get("jt_no_table").unwrap_or(&0) + 1,
            );
            self.jt_missing.insert(br_pc, reg);
            return None;
        }
        let (count, bounded) = match bounds
            .get(&index)
            .copied()
            .filter(|count| *count > 0 && *count <= 4096)
        {
            Some(count) => (count, true),
            None => (if size == 1 { 256 } else { 1024 }, false),
        };
        self.last_table_debug = Some(format!(
            "base={base:x} shift={shift} count={count} size={size} index={index} tables={tables:x?}"
        ));
        let (lo, hi) = self.function_extent(br_pc);
        let mut targets =
            self.read_table_targets(&tables, base, shift, count, size, signed, lo, hi);
        if targets.is_empty() && bounded {
            targets = self.read_table_targets(&tables, base, shift, 1024, size, signed, lo, hi);
        }
        if targets.is_empty() {
            self.stats.insert(
                "jt_no_targets",
                self.stats.get("jt_no_targets").unwrap_or(&0) + 1,
            );
            self.jt_missing.insert(br_pc, reg);
            return None;
        }
        let key = if bounded {
            "jt_bounded"
        } else {
            "jt_unbounded"
        };
        self.stats
            .insert(key, self.stats.get(key).unwrap_or(&0) + 1);
        if at_load {
            self.stats.insert(
                "jt_table_reg_reused",
                self.stats.get("jt_table_reg_reused").unwrap_or(&0) + 1,
            );
        }
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
                    Kind::Br(_) => {
                        if let Some(targets) = self.jt.get(&pc) {
                            for target in targets.iter().copied() {
                                visit(target);
                            }
                        }
                        break;
                    }
                    Kind::Ret(_) | Kind::Trap => break,
                    _ => pc = pc.wrapping_add(4),
                }
            }
        }
        (body, labels)
    }

    /// C for one discovered function. This is the plain control-flow emission;
    /// the flag, frame and local-register transforms follow separately.
    /// C with only the local-register pass applied, before page marks:
    /// nx2/chunks.py applies the game's patches in this state and groups the
    /// marks afterwards.
    pub fn emit_function_unmarked(&self, entry: u64, texts: &[String]) -> String {
        let Some(body) = self.funcs.get(&entry) else {
            return String::new();
        };
        let labels = &self.labels[&entry];
        let mut out = vec![format!(
            "void f_{entry:08x}(Ctx *__restrict A64_NOESC c) {{"
        )];
        let pcs: Vec<u64> = body.keys().copied().collect();
        let processed: &[String] = texts;
        let mut frame = true;
        for (index, pc) in pcs.iter().copied().enumerate() {
            if matches!(body[&pc], Kind::Op) {
                let text = processed.get(index).map(String::as_str).unwrap_or("");
                if frame && !crate::text::fp_is_frame(text) {
                    frame = false;
                }
            }
        }
        let dead = self.dead_flags(entry, body, &pcs, processed);
        let dirty = self.dirty_regs(entry, body, &pcs, processed);
        for (index, pc) in pcs.iter().copied().enumerate() {
            let kind = body[&pc];
            let word = self.code.word(pc).unwrap_or(0);
            if labels.contains(&pc) || (pc == entry && labels.contains(&entry)) {
                out.push(format!("L_{pc:08x}:;"));
            }
            let mut falls = true;
            match kind {
                Kind::Op => {
                    let text = processed.get(index).map(String::as_str).unwrap_or("");
                    if text.contains("recomp_unhandled") {
                        out.push(format!(
                            "\x01A{index}\x01a64_trap(c, 0x{pc:x}ULL, 0x{word:08x}u); return;"
                        ));
                        falls = false;
                    } else {
                        let text = dead
                            .get(&pc)
                            .map(|mask| crate::text::drop_flag_stores(text, *mask))
                            .unwrap_or_else(|| text.to_owned());
                        out.push(text.trim_end_matches('\n').to_owned());
                        if let Some(after) = self.after.get(&pc) {
                            out.push(after.clone());
                        }
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
                        "if ({}{value}) {{ {} }}",
                        if nz != 0 { "" } else { "!" },
                        self.transfer(pc, target, body)
                    ));
                }
                Kind::Bl(target) => {
                    let next = pc + 4;
                    let callee = self.callee(target);
                    if self.entries.contains(&callee) {
                        out.push(format!("\x01C{index}\x01c->x[30] = 0x{next:x}ULL; f_{callee:08x}(c);\x01B{index}\x01"));
                    } else {
                        out.push(format!(
                            "\x01A{index}\x01a64_trap(c, 0x{pc:x}ULL, 0x{word:08x}u); return;"
                        ));
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
                    out.push(format!("\x01R{index}\x01return;"));
                    falls = false;
                }
                Kind::Br(reg) => {
                    if let Some(targets) = self.jt.get(&pc) {
                        let mut sorted: Vec<u64> = targets.iter().copied().collect();
                        sorted.sort_unstable();
                        sorted.dedup();
                        let cases = sorted
                            .iter()
                            .map(|target| {
                                format!(
                                    "case 0x{target:x}ULL: {}",
                                    self.transfer(pc, *target, body)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        out.push(format!(
                            "switch (c->x[{reg}]) {{ {cases} default: A64_IJUMP(c, c->x[{reg}]); return; }}"
                        ));
                    } else {
                        out.push(format!(
                            "\x01A{index}\x01A64_IJUMP(c, c->x[{reg}]); return;"
                        ));
                    }
                    falls = false;
                }
                Kind::Svc(imm) => out.push(format!(
                    "\x01A{index}\x01a64_svc(c, {imm}u);\x01L{index}\x01"
                )),
                Kind::Trap => {
                    out.push(format!(
                        "\x01A{index}\x01a64_trap(c, 0x{pc:x}ULL, 0x{word:08x}u); return;"
                    ));
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
        let mut text = out.join("\n") + "\n";
        if frame {
            text = crate::text::frame_stores(&text);
        }
        crate::text::localize_regs(&text, crate::text::local_count(), (&dirty.0, &dirty.1))
    }

    pub fn emit_function(&self, entry: u64, texts: &[String]) -> String {
        let processed: Vec<String> = texts
            .iter()
            .map(|text| crate::text::op_text(text))
            .collect();
        crate::text::mark_groups(&self.emit_function_unmarked(entry, &processed))
    }

    fn dirty_regs(
        &self,
        entry: u64,
        body: &BTreeMap<u64, Kind>,
        pcs: &[u64],
        texts: &[String],
    ) -> (Vec<u32>, Vec<u32>) {
        let n = pcs.len();
        let at: std::collections::HashMap<u64, usize> =
            pcs.iter().enumerate().map(|(i, pc)| (*pc, i)).collect();
        let mut writes = vec![0u32; n];
        let mut sync = vec![false; n];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, pc) in pcs.iter().copied().enumerate() {
            let kind = body[&pc];
            let mut targets = Vec::new();
            let mut falls = true;
            match kind {
                Kind::Op => {
                    let text = texts.get(i).map(String::as_str).unwrap_or("");
                    if text.is_empty() {
                        falls = false;
                    } else {
                        writes[i] = crate::text::reg_writes(text);
                    }
                }
                Kind::B(target) => {
                    targets.push(target);
                    falls = false;
                }
                Kind::BCond(target, _)
                | Kind::Cbz(target, _, _, _)
                | Kind::Tbz(target, _, _, _) => targets.push(target),
                Kind::Bl(_) | Kind::Blr(_) | Kind::Svc(_) => sync[i] = true,
                Kind::Ret(_) | Kind::Trap => falls = false,
                Kind::Br(_) => {
                    targets.extend(self.jt.get(&pc).into_iter().flatten().copied());
                    falls = false;
                }
                _ => {}
            }
            if falls {
                targets.push(pc + 4);
            }
            for target in targets {
                if (target != entry && body.contains_key(&target)) || target == entry {
                    if let Some(j) = at.get(&target) {
                        succ[i].push(*j);
                    }
                }
            }
        }
        let mut dirty = vec![0u32; n];
        loop {
            let mut changed = false;
            for i in 0..n {
                let out = if sync[i] { 0 } else { dirty[i] | writes[i] };
                for j in &succ[i] {
                    if out & !dirty[*j] != 0 {
                        dirty[*j] |= out;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let after = (0..n)
            .map(|i| if sync[i] { 0 } else { dirty[i] | writes[i] })
            .collect();
        (dirty, after)
    }

    fn dead_flags(
        &self,
        entry: u64,
        body: &BTreeMap<u64, Kind>,
        pcs: &[u64],
        texts: &[String],
    ) -> BTreeMap<u64, u8> {
        const COND_READS: [u8; 16] = [4, 4, 2, 2, 8, 8, 1, 1, 6, 6, 9, 9, 13, 13, 0, 0];
        let n = pcs.len();
        let at: std::collections::HashMap<u64, usize> =
            pcs.iter().enumerate().map(|(i, pc)| (*pc, i)).collect();
        let mut reads = vec![0u8; n];
        let mut kills = vec![0u8; n];
        let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut exits = vec![0u8; n];
        for (i, pc) in pcs.iter().copied().enumerate() {
            let kind = body[&pc];
            let mut targets = Vec::new();
            let mut falls = true;
            match kind {
                Kind::Op => {
                    let text = texts.get(i).map(String::as_str).unwrap_or("");
                    let (r, w) = crate::text::flag_masks(text);
                    reads[i] = r;
                    kills[i] = w;
                }
                Kind::B(target) => {
                    targets.push(target);
                    falls = false;
                }
                Kind::BCond(target, cond) => {
                    reads[i] = COND_READS[cond as usize];
                    targets.push(target);
                }
                Kind::Cbz(target, _, _, _) | Kind::Tbz(target, _, _, _) => targets.push(target),
                Kind::Bl(target) => {
                    if !self.entries.contains(&target) {
                        reads[i] = 15;
                        falls = false;
                    } else {
                        kills[i] = 15;
                    }
                }
                Kind::Blr(_) => kills[i] = 15,
                Kind::Ret(_) => falls = false,
                Kind::Br(_) => {
                    targets.extend(self.jt.get(&pc).into_iter().flatten().copied());
                    falls = false;
                    exits[i] = 15;
                }
                Kind::Svc(_) => reads[i] = 15,
                Kind::Trap => {
                    reads[i] = 15;
                    falls = false;
                }
                _ => {}
            }
            if falls {
                targets.push(pc + 4);
            }
            for target in targets {
                if target != entry && body.contains_key(&target) || target == entry {
                    if let Some(j) = at.get(&target) {
                        succ[i].push(*j);
                    } else {
                        exits[i] = 15;
                    }
                } else {
                    exits[i] = 15;
                }
            }
        }
        let mut live = vec![0u8; n];
        loop {
            let mut changed = false;
            for i in (0..n).rev() {
                let mut out = exits[i];
                for j in &succ[i] {
                    out |= live[*j];
                }
                let value = reads[i] | (out & !kills[i]);
                if value != live[i] {
                    live[i] = value;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let mut dead = BTreeMap::new();
        for (i, pc) in pcs.iter().copied().enumerate() {
            if kills[i] == 0 {
                continue;
            }
            let mut out = exits[i];
            for j in &succ[i] {
                out |= live[*j];
            }
            let mask = kills[i] & !out;
            if mask != 0 {
                dead.insert(pc, mask);
            }
        }
        dead
    }

    fn transfer(&self, pc: u64, target: u64, body: &BTreeMap<u64, Kind>) -> String {
        if body.contains_key(&target) && (!self.entries.contains(&target) || target == pc) {
            format!("goto L_{target:08x};")
        } else if self.entries.contains(&target) {
            format!("TAILCALL(f_{:08x});", self.callee(target))
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

type FpxUse = (String, String, usize, usize);

fn fpx_call() -> &'static regex::Regex {
    static CALL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    CALL.get_or_init(|| {
        regex::Regex::new(r"a64_fpx_([a-z0-9]+)_([a-z0-9]+)\(c,([^)]*?)\)").expect("fpx call")
    })
}

fn collect_fpx(text: &str, used: &mut std::collections::HashSet<FpxUse>) {
    for caps in fpx_call().captures_iter(text) {
        let fast = caps[1].to_owned();
        let exact = caps[2].to_owned();
        let args = caps[3]
            .split(',')
            .filter(|arg| !arg.trim().is_empty())
            .count();
        let nf = if fast.starts_with("fma") {
            3
        } else if fast.starts_with("sqrt") {
            1
        } else {
            2
        };
        used.insert((fast, exact, nf, args));
    }
}

/// Rebuild `a64_fpx.h` from chunks that are already on disk, without
/// re-emitting them. The full lift predates header generation.
pub fn fpx_json(dir: &str) -> Result<serde_json::Value, String> {
    let dir = std::path::Path::new(dir);
    let mut used = std::collections::HashSet::new();
    let mut files = 0usize;
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("code_") && name.ends_with(".c") {
            let text = std::fs::read_to_string(entry.path())
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
            collect_fpx(&text, &mut used);
            files += 1;
        }
    }
    let path = dir.join("a64_fpx.h");
    std::fs::write(&path, crate::text::fpx_header(&used)).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"files": files, "helpers": used.len(), "header": path.to_string_lossy()}))
}

fn load_chunk_boundaries(dir: &std::path::Path) -> Vec<u64> {
    let Ok(text) = std::fs::read_to_string(dir.join("chunks.txt")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Ok(value) = u64::from_str_radix(line, 16) {
            out.push(value);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Boundaries from a size-chunked lift made before chunks.txt existed: the
/// first function defined in each code_NNN.c after the first.
fn seed_chunk_boundaries(dir: &std::path::Path) -> Vec<u64> {
    let head = regex::Regex::new(r"^void f_([0-9a-f]{8})\(Ctx \*__restrict A64_NOESC c\) \{")
        .expect("chunk head regex");
    let mut out = Vec::new();
    for number in 1usize.. {
        let path = dir.join(format!("code_{number:03}.c"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            break;
        };
        if let Some(caps) = text.lines().find_map(|line| head.captures(line)) {
            if let Ok(value) = u64::from_str_radix(&caps[1], 16) {
                out.push(value);
            }
        }
    }
    out
}

fn save_chunk_boundaries(dir: &std::path::Path, starts: &[u64]) -> Result<(), String> {
    let mut text = String::from("# first guest address of each lifted C chunk after the first\n");
    for start in starts.iter().skip(1) {
        text.push_str(&format!("{start:08x}\n"));
    }
    std::fs::write(dir.join("chunks.txt"), text)
        .map_err(|e| format!("{}: {e}", dir.join("chunks.txt").display()))
}

fn chunk_header(decls: &std::collections::BTreeSet<u64>) -> String {
    let mut text = String::from("#include \"a64.h\"\n");
    for entry in decls {
        text.push_str(&format!("void f_{entry:08x}(Ctx *A64_NOESC);\n"));
    }
    text
}

#[allow(clippy::too_many_arguments)]
fn write_chunk(
    dir: &std::path::Path,
    file: &mut usize,
    chunk: &mut String,
    decls: &mut std::collections::BTreeSet<u64>,
    files: &mut Vec<String>,
    starts: &mut Vec<u64>,
    chunk_start: &mut Option<u64>,
    _reference: &regex::Regex,
) -> Result<(), String> {
    if chunk.is_empty() {
        return Ok(());
    }
    if let Some(start) = chunk_start.take() {
        starts.push(start);
    }
    let path = dir.join(format!("code_{:03}.c", *file));
    let body = format!("{}{}", chunk_header(decls), chunk);
    std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    files.push(path.to_string_lossy().into_owned());
    *file += 1;
    chunk.clear();
    decls.clear();
    Ok(())
}

fn remove_stale_chunks(dir: &std::path::Path, keep: usize) -> Result<(), String> {
    let pattern = regex::Regex::new(r"^code_(\d{3})\.c$").expect("chunk name regex");
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(caps) = pattern.captures(&name) {
            if let Ok(number) = caps[1].parse::<usize>() {
                if number >= keep {
                    std::fs::remove_file(entry.path())
                        .map_err(|e| format!("{}: {e}", entry.path().display()))?;
                }
            }
        }
    }
    Ok(())
}

fn codegen_fingerprint() -> Vec<u8> {
    let mut digest = Sha1::new();
    digest.update(b"hookshot-lift-cache-v1");
    for source in [
        include_bytes!("text.rs").as_slice(),
        include_bytes!("insn.rs").as_slice(),
        include_bytes!("emitter.rs").as_slice(),
        include_bytes!("a64.rs").as_slice(),
        include_bytes!("oot_after.rs").as_slice(),
        include_bytes!("lift.rs").as_slice(),
    ] {
        digest.update(source);
        digest.update([0]);
    }
    digest.update(crate::emitter::fingerprint().as_bytes());
    digest.finalize().to_vec()
}

fn env_fingerprint() -> Vec<u8> {
    let mut digest = Sha1::new();
    for name in [
        "A64_LOCAL_REGS",
        "A64_NO_VFPX",
        "A64_MARK_EACH",
        "A64_NO_VSET",
    ] {
        digest.update(name.as_bytes());
        digest.update(b"=");
        digest.update(std::env::var(name).unwrap_or_default().as_bytes());
        digest.update([0]);
    }
    digest.finalize().to_vec()
}

fn hash_resolution(hash: &mut Sha1, lift: &Lifter<Image>, kind: Kind, pc: u64) {
    let target = match kind {
        Kind::B(target)
        | Kind::Bl(target)
        | Kind::BCond(target, _)
        | Kind::Cbz(target, _, _, _)
        | Kind::Tbz(target, _, _, _) => Some(target),
        _ => None,
    };
    if let Some(target) = target {
        hash.update(target.to_le_bytes());
        hash.update([lift.entries.contains(&target) as u8]);
        hash.update(lift.callee(target).to_le_bytes());
    }
    if matches!(kind, Kind::Br(_)) {
        let mut targets = lift.jt.get(&pc).cloned().unwrap_or_default();
        targets.sort_unstable();
        targets.dedup();
        for target in targets {
            hash.update(target.to_le_bytes());
            hash.update([lift.entries.contains(&target) as u8]);
            hash.update(lift.callee(target).to_le_bytes());
        }
    }
}

fn hash_kind(hash: &mut Sha1, kind: Kind) {
    match kind {
        Kind::B(target) => {
            hash.update([1]);
            hash.update(target.to_le_bytes());
        }
        Kind::Bl(target) => {
            hash.update([2]);
            hash.update(target.to_le_bytes());
        }
        Kind::BCond(target, cond) => {
            hash.update([3]);
            hash.update(target.to_le_bytes());
            hash.update([cond]);
        }
        Kind::Cbz(target, reg, sf, nz) => {
            hash.update([4]);
            hash.update(target.to_le_bytes());
            hash.update([reg, sf, nz]);
        }
        Kind::Tbz(target, reg, bit, nz) => {
            hash.update([5]);
            hash.update(target.to_le_bytes());
            hash.update([reg, bit, nz]);
        }
        Kind::Br(reg) => {
            hash.update([6, reg]);
        }
        Kind::Blr(reg) => {
            hash.update([7, reg]);
        }
        Kind::Ret(reg) => {
            hash.update([8, reg]);
        }
        Kind::Svc(imm) => {
            hash.update([9]);
            hash.update(imm.to_le_bytes());
        }
        Kind::Trap => {
            hash.update([10]);
        }
        Kind::Isb => {
            hash.update([11]);
        }
        Kind::Ic => {
            hash.update([12]);
        }
        Kind::Op => {
            hash.update([13]);
        }
    }
}

fn function_key(
    lift: &Lifter<Image>,
    image: &Image,
    entry: u64,
    codegen: &[u8],
    env: &[u8],
) -> [u8; 20] {
    let mut hash = Sha1::new();
    hash.update(b"hookshot-lift-cache-fn-v1");
    hash.update(codegen);
    hash.update(env);
    hash.update(entry.to_le_bytes());
    if let Some((target, slot)) = lift.plt.get(&entry) {
        hash.update([1]);
        hash.update(target.to_le_bytes());
        hash.update(slot.to_le_bytes());
    } else {
        hash.update([0]);
    }
    if let Some(body) = lift.funcs.get(&entry) {
        for (pc, kind) in body {
            hash.update(pc.to_le_bytes());
            hash.update(image.word(*pc).unwrap_or(0).to_le_bytes());
            hash_kind(&mut hash, *kind);
            hash_resolution(&mut hash, lift, *kind, *pc);
        }
    }
    let mut labels: Vec<u64> = lift
        .labels
        .get(&entry)
        .into_iter()
        .flatten()
        .copied()
        .collect();
    labels.sort_unstable();
    for label in labels {
        hash.update(label.to_le_bytes());
    }
    hash.finalize().into()
}

/// The already transformed text of each distinct instruction: shared words
/// by value, address-naming instructions by address.
#[derive(Default)]
struct OpTexts {
    word: std::collections::HashMap<u32, String>,
    at: std::collections::HashMap<(u64, u32), String>,
}

fn translate_in_batches(items: &[(u64, u32)]) -> Result<(Vec<String>, Vec<i32>), String> {
    let mut texts = Vec::with_capacity(items.len());
    let mut flags = Vec::with_capacity(items.len());
    // (a batch of tens of thousands needs a few megabytes, not the 16 MB the
    // shared translator used to allocate even for a three-instruction call)
    for chunk in items.chunks(32 * 1024) {
        let (part, part_flags) = crate::emitter::translate(chunk)?;
        texts.extend(part);
        flags.extend(part_flags);
    }
    Ok((texts, flags))
}

/// Translate every distinct instruction of the functions that missed the
/// cache once, instead of calling the shared translator once per function and
/// running the text transforms per occurrence. An instruction's text may name
/// its own address (branches, ADR/ADRP, unhandled words); those keep one entry
/// per address. Every other word is shared across the whole image.
fn shared_texts<C: Code>(
    lift: &Lifter<C>,
    image: &C,
    missing: &[(usize, u64, [u8; 20])],
) -> Result<OpTexts, String> {
    use std::collections::{HashMap, HashSet};
    let mut words: HashSet<u32> = HashSet::new();
    let mut address_ops: HashSet<(u64, u32)> = HashSet::new();
    for (_, entry, _) in missing {
        let Some(body) = lift.funcs.get(entry) else {
            continue;
        };
        for (pc, kind) in body {
            if !matches!(kind, Kind::Op) {
                continue;
            }
            let Some(word) = image.word(*pc) else {
                continue;
            };
            if a64::pc_relative(word) {
                address_ops.insert((*pc, word));
            } else {
                words.insert(word);
            }
        }
    }
    let at_zero: Vec<(u64, u32)> = words.iter().map(|word| (0, *word)).collect();
    let at_four: Vec<(u64, u32)> = words.iter().map(|word| (4, *word)).collect();
    let (zero, flags) = translate_in_batches(&at_zero)?;
    let (four, _) = translate_in_batches(&at_four)?;
    // (the shared translator's unhandled flag names the address; the second
    // address catches any other text that follows it)
    let mut address_sensitive: HashSet<u32> = HashSet::new();
    for (index, word) in words.iter().copied().enumerate() {
        if flags[index] & 1 != 0 || zero[index] != four[index] {
            address_sensitive.insert(word);
        }
    }
    if !address_sensitive.is_empty() {
        for (_, entry, _) in missing {
            let Some(body) = lift.funcs.get(entry) else {
                continue;
            };
            for (pc, kind) in body {
                if !matches!(kind, Kind::Op) {
                    continue;
                }
                if let Some(word) = image.word(*pc) {
                    if address_sensitive.contains(&word) {
                        address_ops.insert((*pc, word));
                    }
                }
            }
        }
    }
    let mut addresses: Vec<(u64, u32)> = address_ops.into_iter().collect();
    addresses.sort_unstable();
    let (address_text, _) = translate_in_batches(&addresses)?;
    let mut out = OpTexts::default();
    let mut transformed: HashMap<String, String> = HashMap::new();
    let text_of = |raw: &str, transformed: &mut HashMap<String, String>| {
        transformed
            .entry(raw.to_owned())
            .or_insert_with(|| crate::text::op_text(raw))
            .clone()
    };
    for ((pc, word), raw) in addresses.iter().zip(address_text) {
        let text = text_of(&raw, &mut transformed);
        out.at.insert((*pc, *word), text);
    }
    for (index, word) in words.iter().copied().enumerate() {
        if address_sensitive.contains(&word) {
            continue;
        }
        let text = text_of(&zero[index], &mut transformed);
        out.word.insert(word, text);
    }
    Ok(out)
}

fn processed_texts<C: Code>(
    lift: &Lifter<C>,
    image: &C,
    entry: u64,
    texts: &OpTexts,
) -> Vec<String> {
    let Some(body) = lift.funcs.get(&entry) else {
        return Vec::new();
    };
    body.iter()
        .map(|(pc, kind)| {
            if !matches!(kind, Kind::Op) {
                return String::new();
            }
            let word = image.word(*pc).unwrap_or(0);
            texts
                .at
                .get(&(*pc, word))
                .or_else(|| texts.word.get(&word))
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

fn emit_entry<C: Code>(
    lift: &Lifter<C>,
    image: &C,
    entry: u64,
    texts: &OpTexts,
) -> Result<String, String> {
    let processed = processed_texts(lift, image, entry, texts);
    Ok(lift.emit_function_unmarked(entry, &processed))
}

fn mark_one(path: &std::path::Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let marked = crate::text::mark_groups(&text);
    if marked != text {
        std::fs::write(path, marked).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn mark_chunk_files(dir: &std::path::Path) -> Result<(), String> {
    let pattern = regex::Regex::new(r"^code_\d{3}\.c$").expect("chunk name regex");
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if pattern.is_match(&entry.file_name().to_string_lossy()) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    let workers = std::env::var("HOOKSHOT_LIFT_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16)
        .max(1)
        .min(paths.len().max(1));
    if workers <= 1 {
        for path in &paths {
            mark_one(path)?;
        }
        return Ok(());
    }
    let per = paths.len().div_ceil(workers);
    std::thread::scope(|scope| -> Result<(), String> {
        let handles: Vec<_> = paths
            .chunks(per)
            .map(|chunk| scope.spawn(move || chunk.iter().try_for_each(|path| mark_one(path))))
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| "a mark worker stopped".to_string())??;
        }
        Ok(())
    })
}

pub fn write_json(
    exefs: &str,
    out: &str,
    limit: usize,
    chunk_bytes: usize,
    standalone: bool,
) -> Result<serde_json::Value, String> {
    let path = std::path::Path::new(exefs);
    let total = crate::timing::Stage::new("lift.total");
    let read = crate::timing::Stage::new("lift.read");
    let hle = if standalone {
        crate::standalone::hle_symbols(exefs)?
    } else {
        Vec::new()
    };
    let mut image = crate::image::Image::build(path, 0, false, &hle, &[])?;
    let (adr_refs, adr_new, adr_spans) = image.add_adr_entries(4);
    read.done();
    let discover = crate::timing::Stage::new("lift.discover");
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.hooks = image.hooks.clone();
    lift.entries.extend(lift.hooks.keys().copied());
    // (a64_lift.Lifter: img.eh_starts, the FDE boundaries, not every entry:
    // they bound jump-table target validation)
    lift.eh_starts = image.eh_starts().to_vec();
    lift.names = image.names().clone();
    lift.discover();
    discover.done();
    if standalone {
        let after = crate::timing::Stage::new("lift.after_sites");
        lift.after = crate::oot_after::sites(&image);
        after.done();
    }
    let out = std::path::Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let entries: Vec<u64> = lift
        .funcs
        .keys()
        .copied()
        .take(if limit == 0 { usize::MAX } else { limit })
        .collect();
    let cache_dir = out.join("lift_cache");
    let cache_open = crate::timing::Stage::new("lift.cache_open");
    let mut cache = crate::lift_cache::Cache::open(&cache_dir)?;
    cache_open.done();
    let codegen = codegen_fingerprint();
    let mut env = env_fingerprint();
    if !lift.after.is_empty() {
        let mut after = Sha1::new();
        for (pc, text) in &lift.after {
            after.update(pc.to_le_bytes());
            after.update(text.as_bytes());
        }
        env.extend(after.finalize());
    }
    let hash_keys = crate::timing::Stage::new("lift.hash_keys");
    let workers = std::env::var("HOOKSHOT_LIFT_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16)
        .max(1)
        .min(entries.len().max(1));
    let keys: Vec<[u8; 20]> = if workers <= 1 {
        entries
            .iter()
            .map(|entry| function_key(&lift, &image, *entry, &codegen, &env))
            .collect()
    } else {
        let per = entries.len().div_ceil(workers);
        std::thread::scope(|scope| {
            let handles: Vec<_> = entries
                .chunks(per)
                .map(|chunk| {
                    let lift = &lift;
                    let image = &image;
                    let codegen = &codegen;
                    let env = &env;
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .map(|entry| function_key(lift, image, *entry, codegen, env))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut keys = Vec::with_capacity(entries.len());
            for handle in handles {
                keys.extend(handle.join().unwrap_or_default());
            }
            keys
        })
    };
    hash_keys.done();
    let lookup = crate::timing::Stage::new("lift.cache_lookup");
    let mut emitted: Vec<Option<String>> = vec![None; entries.len()];
    let mut misses: Vec<(usize, u64, [u8; 20])> = Vec::new();
    let mut hits = 0usize;
    let looked_up: Vec<(usize, Option<String>)> = if workers <= 1 {
        keys.iter()
            .enumerate()
            .map(|(index, key)| (index, cache.get(key)))
            .collect()
    } else {
        let per = keys.len().div_ceil(workers);
        std::thread::scope(|scope| {
            let handles: Vec<_> = keys
                .chunks(per)
                .enumerate()
                .map(|(part, chunk)| {
                    let cache = &cache;
                    let base = part * per;
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .enumerate()
                            .map(|(index, key)| (base + index, cache.get(key)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut out = Vec::with_capacity(keys.len());
            for handle in handles {
                out.extend(handle.join().unwrap_or_default());
            }
            out
        })
    };
    for (index, text) in looked_up {
        if let Some(text) = text {
            emitted[index] = Some(text);
            hits += 1;
        } else {
            misses.push((index, entries[index], keys[index]));
        }
    }
    lookup.mark(&format!("{hits} cache hits, {} misses", misses.len()));
    lookup.done();
    let texts = crate::timing::span("lift.shared_texts", || {
        shared_texts(&lift, &image, &misses)
    })?;
    let emit = crate::timing::Stage::new("lift.emit");
    let workers = std::env::var("HOOKSHOT_LIFT_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(16)
        .max(1)
        .min(misses.len().max(1));
    let results: Vec<(usize, [u8; 20], Result<String, String>)> = if workers <= 1 {
        let mut out = Vec::new();
        for &(index, entry, key) in &misses {
            out.push((index, key, emit_entry(&lift, &image, entry, &texts)));
        }
        out
    } else {
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let lift = &lift;
                    let image = &image;
                    let misses = &misses;
                    let texts = &texts;
                    scope.spawn(move || {
                        let mut out = Vec::new();
                        let mut at = worker;
                        while at < misses.len() {
                            let (index, entry, key) = misses[at];
                            out.push((index, key, emit_entry(lift, image, entry, &texts)));
                            at += workers;
                        }
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap_or_default())
                .collect()
        })
    };
    emit.done();
    let cache_store = crate::timing::Stage::new("lift.cache_store");
    for (index, key, result) in results {
        let text = result?;
        cache.put(key, &text)?;
        emitted[index] = Some(text);
    }
    if !misses.is_empty() {
        cache.save()?;
    }
    cache_store.done();
    if emitted.iter().any(Option::is_none) {
        return Err("the lift cache left some functions without C".into());
    }
    let mut emitted: Vec<(usize, String)> = emitted
        .into_iter()
        .enumerate()
        .filter_map(|(index, text)| text.map(|text| (index, text)))
        .collect();
    emitted.sort_by_key(|(index, _)| *index);
    let fpx = crate::timing::Stage::new("lift.fpx_collect");
    let mut used = std::collections::HashSet::new();
    for (_, text) in &emitted {
        collect_fpx(text, &mut used);
    }
    std::fs::write(out.join("a64_fpx.h"), crate::text::fpx_header(&used))
        .map_err(|e| e.to_string())?;
    fpx.done();
    let written = entries.len();
    let mut written_entries = entries.clone();
    written_entries.extend(lift.hooks.keys().copied());
    written_entries.sort_unstable();
    written_entries.dedup();
    let reference = regex::Regex::new(r"\bf_([0-9a-f]{8})\b").unwrap();
    let boundaries = {
        let saved = load_chunk_boundaries(out);
        if saved.is_empty() {
            seed_chunk_boundaries(out)
        } else {
            saved
        }
    };
    let chunks = crate::timing::Stage::new("lift.chunks");
    let mut chunk = String::new();
    let mut decls = std::collections::BTreeSet::new();
    let mut chunk_start: Option<u64> = None;
    let mut size = 0usize;
    let mut file = 0usize;
    let mut files = Vec::new();
    let mut starts = Vec::new();
    let mut next_boundary = 0usize;
    for (index, text) in emitted {
        let entry = entries[index];
        while next_boundary < boundaries.len() && entry >= boundaries[next_boundary] {
            write_chunk(
                out,
                &mut file,
                &mut chunk,
                &mut decls,
                &mut files,
                &mut starts,
                &mut chunk_start,
                &reference,
            )?;
            next_boundary += 1;
            size = 0;
        }
        if chunk_start.is_none() {
            chunk_start = Some(entry);
        }
        for caps in reference.captures_iter(&text) {
            if let Ok(entry) = u64::from_str_radix(&caps[1], 16) {
                decls.insert(entry);
            }
        }
        chunk.push_str(&text);
        size += text.len();
        if boundaries.is_empty() && size >= chunk_bytes {
            write_chunk(
                out,
                &mut file,
                &mut chunk,
                &mut decls,
                &mut files,
                &mut starts,
                &mut chunk_start,
                &reference,
            )?;
            size = 0;
        }
    }
    write_chunk(
        out,
        &mut file,
        &mut chunk,
        &mut decls,
        &mut files,
        &mut starts,
        &mut chunk_start,
        &reference,
    )?;
    chunks.mark("assembled and written");
    save_chunk_boundaries(out, &starts)?;
    remove_stale_chunks(out, files.len())?;
    chunks.done();
    let build = crate::nso::build_id(&path.join("main")).unwrap_or_default();
    let patches = crate::timing::span("lift.patches", || {
        crate::oot_patches::apply_json(
            out.to_str().unwrap_or_default(),
            &build,
            standalone,
            limit == 0,
        )
    })?;
    crate::timing::span("lift.mark_groups", || mark_chunk_files(out))?;
    let tables = crate::timing::Stage::new("lift.tables");
    let mut table = String::from("#include \"a64.h\"\n");
    for entry in &written_entries {
        table.push_str(&format!("void f_{entry:08x}(Ctx *A64_NOESC);\n"));
    }
    table.push_str(&format!(
        "const uint32_t a64_fn_count = {};\nconst uint32_t a64_fn_addr[] = {{\n",
        written_entries.len()
    ));
    for entry in &written_entries {
        table.push_str(&format!("0x{entry:08x},\n"));
    }
    table.push_str("};\nconst A64Fn a64_fn_ptr[] = {\n");
    for entry in &written_entries {
        table.push_str(&format!("f_{entry:08x},\n"));
    }
    table.push_str("};\n");
    std::fs::write(out.join("a64_table.c"), table).map_err(|e| e.to_string())?;
    let mut hooks_c = String::from(
        "/* Hooked guest addresses: f_<address> is the host function (an HLE or a generated stub). */\n#include \"a64.h\"\n",
    );
    let mut names: Vec<&String> = lift.hooks.values().collect();
    names.sort();
    names.dedup();
    for name in &names {
        hooks_c.push_str(&format!("void {name}(Ctx *A64_NOESC);\n"));
    }
    for (address, name) in &lift.hooks {
        hooks_c.push_str(&format!(
            "void f_{address:08x}(Ctx *A64_NOESC c) {{ TAILCALL({name}); }}\n"
        ));
    }
    std::fs::write(out.join("a64_hooks.c"), hooks_c).map_err(|e| e.to_string())?;
    tables.done();
    total.done();
    Ok(serde_json::json!({
        "files": files,
        "functions": written,
        "hooks": lift.hooks.len(),
        "adr_refs": adr_refs,
        "adr_new": adr_new,
        "adr_spans": adr_spans,
        "boundaries": starts.len(),
        "after": lift.after.len(),
        "patches": patches,
    }))
}
pub fn emit_json(exefs: &str, entry: u64) -> Result<serde_json::Value, String> {
    let standalone = std::env::var("HOOKSHOT_EMIT_STANDALONE").ok().as_deref() == Some("1");
    let hle = if standalone {
        crate::standalone::hle_symbols(exefs)?
    } else {
        Vec::new()
    };
    let mut image = crate::image::Image::build(std::path::Path::new(exefs), 0, false, &hle, &[])?;
    if standalone {
        image.add_adr_entries(4);
    }
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.eh_starts = image.eh_starts().to_vec();
    lift.names = image.names().clone();
    lift.discover();
    let entry = if lift.funcs.contains_key(&entry) {
        entry
    } else {
        *lift.funcs.keys().next().ok_or("no functions")?
    };
    let body = &lift.funcs[&entry];
    let items: Vec<(u64, u32)> = body
        .keys()
        .filter_map(|pc| image.word(*pc).map(|word| (*pc, word)))
        .collect();
    let (translated, _) = crate::emitter::translate(&items)?;
    let by_pc: std::collections::HashMap<u64, String> =
        items.iter().map(|(pc, _)| *pc).zip(translated).collect();
    let texts: Vec<String> = body
        .keys()
        .map(|pc| crate::text::op_text(&by_pc.get(pc).cloned().unwrap_or_default()))
        .collect();
    let frame_false: Vec<String> = body
        .keys()
        .zip(texts.iter())
        .filter(|(pc, text)| matches!(body[*pc], Kind::Op) && !crate::text::fp_is_frame(text))
        .map(|(pc, _)| format!("{pc:x}"))
        .collect();
    let pcs: Vec<u64> = body.keys().copied().collect();
    let dead = lift.dead_flags(entry, body, &pcs, &texts);
    let v_ops: Vec<(String, u8, u8, u8)> = pcs
        .iter()
        .zip(texts.iter())
        .filter(|(_, text)| text.contains("c->v="))
        .map(|(pc, text)| {
            let transformed = crate::text::op_text(text);
            let (reads, writes) = crate::text::flag_masks(&transformed);
            (
                format!("{pc:x}"),
                dead.get(pc).copied().unwrap_or(0),
                reads,
                writes,
            )
        })
        .collect();
    let branches: Vec<(String, Vec<String>)> = body
        .iter()
        .filter_map(|(pc, kind)| match kind {
            Kind::Br(_) => Some((
                format!("{pc:x}"),
                lift.jt
                    .get(pc)
                    .map(|targets| targets.iter().map(|t| format!("{t:x}")).collect())
                    .unwrap_or_default(),
            )),
            _ => None,
        })
        .collect();
    Ok(
        serde_json::json!({"entry": format!("{entry:x}"), "frame_false": frame_false, "v_ops": v_ops, "branches": branches, "c": lift.emit_function_unmarked(entry, &texts)}),
    )
}

pub fn coverage_json(exefs: &str, pcs: &[u64]) -> Result<serde_json::Value, String> {
    let image = crate::image::Image::build(std::path::Path::new(exefs), 0, false, &[], &[])?;
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.eh_starts = image.eh_starts().to_vec();
    lift.names = image.names().clone();
    lift.discover();
    let rows: Vec<serde_json::Value> = pcs
        .iter()
        .map(|pc| {
            let entry = lift
                .funcs
                .iter()
                .find(|(_, body)| body.contains_key(pc))
                .map(|(entry, _)| *entry);
            serde_json::json!({
                "pc": format!("{pc:x}"),
                "entry": entry.map(|e| format!("{e:x}")),
                "in_entries": image.entries.contains(pc),
                "in_code": image.in_code(*pc),
                "in_body": entry.is_some(),
            })
        })
        .collect();
    Ok(serde_json::json!({"rows": rows}))
}

pub fn debug_json(exefs: &str, br: u64, reg: u8, table: u64) -> Result<serde_json::Value, String> {
    let image = crate::image::Image::build(std::path::Path::new(exefs), 0, false, &[], &[])?;
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.eh_starts = image.eh_starts().to_vec();
    lift.names = image.names().clone();
    lift.discover();
    let reaching: Vec<String> = lift
        .reaching(br, Loc::Reg(reg), 50000, 0)
        .into_iter()
        .map(|v| format!("{v:x}"))
        .collect();
    let constants: Vec<String> = lift
        .function_constants(br, reg)
        .into_iter()
        .map(|v| format!("{v:x}"))
        .collect();
    let bytes = image
        .read(table, 16)
        .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .unwrap_or_default();
    let (lo, hi) = lift.function_extent(br);
    let direct: Vec<String> = lift
        .read_table_targets(&[table], 0x82191c50, 2, 46, 2, false, lo, hi)
        .into_iter()
        .map(|t| format!("{t:x}"))
        .collect();
    let targets: Vec<String> = lift
        .jump_table(br, 9)
        .unwrap_or_default()
        .into_iter()
        .map(|t| format!("{t:x}"))
        .collect();
    Ok(serde_json::json!({
        "extent": lift.function_extent(br),
        "hoisted": lift.hoisted_constant(br, reg, 1024).map(|v| format!("{v:x}")),
        "constants": constants,
        "reaching": reaching,
        "table_bytes": bytes,
        "targets": targets,
        "direct": direct,
        "symbolic": lift.last_table_debug,
    }))
}

pub fn tables_json(exefs: &str) -> Result<serde_json::Value, String> {
    let image = crate::image::Image::build(std::path::Path::new(exefs), 0, false, &[], &[])?;
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.eh_starts = image.eh_starts().to_vec();
    lift.names = image.names().clone();
    lift.discover();
    Ok(serde_json::json!({
        "functions": lift.funcs.len(),
        "jump_tables": lift.jt.len(),
        "plt_stubs": lift.plt.len(),
        "missing": lift.jt_missing.keys().map(|pc| format!("{pc:x}")).collect::<Vec<_>>(),
        "keys": lift.jt.keys().map(|pc| format!("{pc:x}")).collect::<Vec<_>>(),
        "func_keys": lift.funcs.keys().map(|pc| format!("{pc:x}")).collect::<Vec<_>>(),
        "missing_in_body": lift.jt_missing.keys().map(|pc| (format!("{pc:x}"), lift.funcs.values().any(|body| body.contains_key(pc)))).collect::<Vec<_>>(),
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

    fn nops(bytes: usize) -> Vec<u32> {
        vec![0xD503_201F; bytes / 4]
    }

    fn put(words: &mut [u32], address: u64, word: u32) {
        words[(address / 4) as usize] = word;
    }

    #[test]
    fn table_base_rewritten_after_the_load_resolves_at_the_load() {
        // adrp/add x10 = table; b dispatch; ...; adr x8 = targets;
        // ldrh w9, [x10, x0, lsl #1]; add x8 = x8 + w9 * 4;
        // adrp/add x10 = a string (a second constant assigned to x10); br x8.
        let mut words = nops(0x400);
        put(&mut words, 0x00, 0x9000_000A); // adrp x10, page 0
        put(&mut words, 0x04, 0x9108_014A); // add  x10, x10, #0x200
        put(&mut words, 0x08, 0x1400_001E); // b    0x80
        put(&mut words, 0xD8, 0x1000_0168); // adr  x8, 0x104
        put(&mut words, 0xE0, 0x7860_7949); // ldrh w9, [x10, x0, lsl #1]
        put(&mut words, 0xE4, 0x8B09_0908); // add  x8, x8, x9, lsl #2
        put(&mut words, 0xE8, 0xB000_000A); // adrp x10, page 1
        put(&mut words, 0xEC, 0x9100_414A); // add  x10, x10, #0x10
        put(&mut words, 0x100, 0xD61F_0100); // br   x8
        put(&mut words, 0x104, 0xD65F_03C0); // ret
        put(&mut words, 0x114, 0xD65F_03C0);
        put(&mut words, 0x124, 0xD65F_03C0);
        put(&mut words, 0x134, 0xD65F_03C0);
        words[0x200 / 4] = 0x0004_0000; // u16 entries 0, 4
        words[0x204 / 4] = 0x000C_0008; // u16 entries 8, 12
        let mock = Mock(words);
        let mut lift = Lifter::new(&mock, [0]);
        lift.eh_starts = vec![0, 0x300];
        let targets = lift.jump_table(0x100, 8).expect("table");
        assert_eq!(targets, vec![0x104, 0x114, 0x124, 0x134]);
    }

    #[test]
    fn table_base_set_in_the_window_survives_a_reuse_after_the_load() {
        // The shape of the seven tables nx2/lift_fixes.py's table_load sees in
        // this game: the base is set in the window, so the symbolic load keeps
        // the address even though the register is reused before the BR.
        let mut words = nops(0x400);
        put(&mut words, 0x80, 0x9000_000A); // adrp x10, page 0
        put(&mut words, 0x84, 0x9108_014A); // add  x10, x10, #0x200
        put(&mut words, 0xD8, 0x1000_0168); // adr  x8, 0x104
        put(&mut words, 0xE0, 0x7860_7949); // ldrh w9, [x10, x0, lsl #1]
        put(&mut words, 0xE4, 0x8B09_0908); // add  x8, x8, x9, lsl #2
        put(&mut words, 0xE8, 0xB000_000A); // adrp x10, page 1
        put(&mut words, 0xEC, 0x9100_414A); // add  x10, x10, #0x10
        put(&mut words, 0x100, 0xD61F_0100); // br   x8
        put(&mut words, 0x104, 0xD65F_03C0);
        put(&mut words, 0x114, 0xD65F_03C0);
        put(&mut words, 0x124, 0xD65F_03C0);
        put(&mut words, 0x134, 0xD65F_03C0);
        words[0x200 / 4] = 0x0004_0000;
        words[0x204 / 4] = 0x000C_0008;
        let mock = Mock(words);
        let mut lift = Lifter::new(&mock, [0]);
        lift.eh_starts = vec![0, 0x300];
        assert_eq!(
            lift.jump_table(0x100, 8),
            Some(vec![0x104, 0x114, 0x124, 0x134])
        );
        assert_eq!(lift.stats.get("jt_table_reg_reused"), None);
    }

    #[test]
    fn an_adr_hoisted_table_is_found_by_the_function_scan() {
        // rtld+0x8a0: adr x25, table before the loop; ldrb; adr x9, targets;
        // add; br x9. The setup is outside the 32-instruction window.
        let mut words = nops(0x400);
        put(&mut words, 0x00, 0x1000_1019); // adr  x25, 0x200
        put(&mut words, 0xE0, 0x3868_6B2A); // ldrb w10, [x25, x8]
        put(&mut words, 0xE4, 0x1000_0109); // adr  x9, 0x104
        put(&mut words, 0xE8, 0x8B0A_0929); // add  x9, x9, x10, lsl #2
        put(&mut words, 0x100, 0xD61F_0120); // br   x9
        put(&mut words, 0x104, 0xD65F_03C0);
        put(&mut words, 0x114, 0xD65F_03C0);
        put(&mut words, 0x124, 0xD65F_03C0);
        put(&mut words, 0x134, 0xD65F_03C0);
        words[0x200 / 4] = 0x0C08_0400; // u8 entries 0, 4, 8, 12
        words[0x204 / 4] = 0x0000_FFFF; // then out of range
        let mock = Mock(words);
        let mut lift = Lifter::new(&mock, [0]);
        lift.eh_starts = vec![0, 0x300];
        assert_eq!(
            lift.jump_table(0x100, 9),
            Some(vec![0x104, 0x114, 0x124, 0x134])
        );
    }

    #[test]
    fn an_authenticated_return_ends_the_symbolic_window() {
        // main+0x22b9b84: the table base is set before a retab, with a
        // different adrp/add pair after the branch away from it. Without the
        // window cut the later pair is the table and no target is in range.
        let mut words = nops(0x400);
        put(&mut words, 0x00, 0x9000_0000); // adrp x0, page 0
        put(&mut words, 0x04, 0x9108_0000); // add  x0, x0, #0x200
        put(&mut words, 0x08, 0x1400_000E); // b    0x40
        put(&mut words, 0x0C, 0xB000_0000); // adrp x0, page 1
        put(&mut words, 0x10, 0x9100_4000); // add  x0, x0, #0x10
        put(&mut words, 0x14, 0xD65F_0FFF); // retab
        put(&mut words, 0x40, 0x1000_0628); // adr  x8, 0x104
        put(&mut words, 0x60, 0x7861_7809); // ldrh w9, [x0, x1, lsl #1]
        put(&mut words, 0x64, 0x8B09_0908); // add  x8, x8, x9, lsl #2
        put(&mut words, 0x68, 0xD61F_0100); // br   x8
        put(&mut words, 0x104, 0xD65F_03C0);
        put(&mut words, 0x114, 0xD65F_03C0);
        put(&mut words, 0x124, 0xD65F_03C0);
        put(&mut words, 0x134, 0xD65F_03C0);
        words[0x200 / 4] = 0x0004_0000; // u16 entries 0, 4
        words[0x204 / 4] = 0x000C_0008; // u16 entries 8, 12
        words[0x208 / 4] = 0x0000_FFFF; // then out of range
        let mock = Mock(words);
        let mut lift = Lifter::new(&mock, [0]);
        lift.eh_starts = vec![0, 0x300];
        assert_eq!(
            lift.jump_table(0x68, 8),
            Some(vec![0x104, 0x114, 0x124, 0x134])
        );
    }

    #[test]
    fn hooked_entries_are_aliases_not_lifted() {
        let mock = Mock(vec![0xD65F_03C0, 0xD65F_03C0]);
        let mut lift = Lifter::new(&mock, [0, 4]);
        lift.hooks.insert(0, "imp_hooked".to_owned());
        lift.entries.insert(0);
        lift.discover();
        assert!(!lift.funcs.contains_key(&0));
        assert!(lift.funcs.contains_key(&4));
    }
    #[test]
    fn dead_flags_after_a_return_are_dropped() {
        let mock = Mock(vec![0x9100_0000, 0xD65F_03C0]);
        let lift = Lifter::new(&mock, [0]);
        let mut body = BTreeMap::new();
        body.insert(0, Kind::Op);
        body.insert(4, Kind::Ret(30));
        let pcs = vec![0, 4];
        let texts = vec!["c->n=1; c->z=2;".to_owned(), "return;".to_owned()];
        let dead = lift.dead_flags(0, &body, &pcs, &texts);
        assert_eq!(dead.get(&0), Some(&12));
        assert_eq!(crate::text::drop_flag_stores(&texts[0], 12), " ");
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
    fn shared_texts_translate_each_word_once() {
        let words = vec![
            0xD503_201F,
            0x9100_0000,
            0xD65F_03C0,
            0xD503_201F,
            0x9100_0000,
            0xD65F_03C0,
        ];
        let mock = Mock(words);
        let mut lift = Lifter::new(&mock, [0, 12]);
        lift.discover();
        let missing: Vec<(usize, u64, [u8; 20])> = vec![(0, 0, [0; 20]), (1, 12, [0; 20])];
        let Ok(texts) = shared_texts(&lift, &mock, &missing) else {
            return; // (no vendored translator here)
        };
        let first = processed_texts(&lift, &mock, 0, &texts);
        let second = processed_texts(&lift, &mock, 12, &texts);
        assert_eq!(first.len(), 3);
        assert_eq!(first, second);
        assert!(first[0].contains("nop"));
        assert!(first[1].contains("c->x[0]"));
        assert!(texts.at.is_empty());
        assert_eq!(texts.word.len(), 2);
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
