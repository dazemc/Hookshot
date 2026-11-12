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
        } = load
        else {
            self.stats.insert(
                "jt_not_load",
                self.stats.get("jt_not_load").unwrap_or(&0) + 1,
            );
            return self.adr_targets(br_pc, reg);
        };
        let tables: Vec<u64> = match *table {
            V::Const(table) => vec![table],
            V::Reg(reg) => {
                let mut tables: Vec<u64> = self
                    .reaching(br_pc, Loc::Reg(reg), 50000, 0)
                    .into_iter()
                    .collect();
                if tables.is_empty() {
                    tables.extend(self.function_constants(br_pc, reg));
                }
                if tables.is_empty() {
                    if let Some(hoisted) = self.hoisted_constant(br_pc, reg, 1024) {
                        tables.push(hoisted);
                    }
                }
                tables.sort_unstable();
                tables.dedup();
                tables
            }
            V::Add { left, right, .. } => match (*left, *right) {
                (V::Reg(reg), V::Const(offset)) => {
                    let mut tables: Vec<u64> = self
                        .reaching(br_pc, Loc::Reg(reg), 50000, 0)
                        .into_iter()
                        .collect();
                    if tables.is_empty() {
                        tables.extend(self.function_constants(br_pc, reg));
                    }
                    if tables.is_empty() {
                        if let Some(hoisted) = self.hoisted_constant(br_pc, reg, 1024) {
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
    pub fn emit_function(&self, entry: u64, texts: &[String]) -> String {
        let Some(body) = self.funcs.get(&entry) else {
            return String::new();
        };
        let labels = &self.labels[&entry];
        let mut out = vec![format!(
            "void f_{entry:08x}(Ctx *__restrict A64_NOESC c) {{"
        )];
        let pcs: Vec<u64> = body.keys().copied().collect();
        let processed: Vec<String> = texts
            .iter()
            .map(|text| crate::text::op_text(text))
            .collect();
        let mut frame = true;
        for (index, pc) in pcs.iter().copied().enumerate() {
            if matches!(body[&pc], Kind::Op) {
                let text = processed.get(index).map(String::as_str).unwrap_or("");
                if frame && !crate::text::fp_is_frame(text) {
                    frame = false;
                }
            }
        }
        let dead = self.dead_flags(entry, body, &pcs, &processed);
        let dirty = self.dirty_regs(entry, body, &pcs, &processed);
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
                    if text.is_empty() || text.contains("recomp_unhandled") {
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
                    out.push(format!(
                        "\x01A{index}\x01A64_IJUMP(c, c->x[{reg}]); return;"
                    ));
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
        let text =
            crate::text::localize_regs(&text, crate::text::local_count(), (&dirty.0, &dirty.1));
        crate::text::mark_groups(&text)
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

fn collect_fpx(text: &str, used: &mut std::collections::HashSet<FpxUse>) {
    let call = regex::Regex::new(r"a64_fpx_([a-z0-9]+)_([a-z0-9]+)\(c,([^)]*?)\)").unwrap();
    for caps in call.captures_iter(text) {
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

pub fn write_json(
    exefs: &str,
    out: &str,
    limit: usize,
    chunk_bytes: usize,
    standalone: bool,
) -> Result<serde_json::Value, String> {
    let path = std::path::Path::new(exefs);
    let hle = if standalone {
        crate::standalone::hle_symbols(exefs)?
    } else {
        Vec::new()
    };
    let image = crate::image::Image::build(path, 0, false, &hle, &[])?;
    let mut lift = Lifter::new(&image, image.entries.iter().copied());
    lift.hooks = image.hooks.clone();
    lift.entries.extend(lift.hooks.keys().copied());
    lift.eh_starts = lift.entries.iter().copied().collect();
    lift.eh_starts.sort_unstable();
    lift.names = image.names().clone();
    lift.discover();
    let out = std::path::Path::new(out);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let entries: Vec<u64> = lift
        .funcs
        .keys()
        .copied()
        .take(if limit == 0 { usize::MAX } else { limit })
        .collect();
    let workers = std::env::var("HOOKSHOT_LIFT_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8)
        .max(1)
        .min(entries.len().max(1));
    let mut emitted: Vec<(usize, String)> = if workers <= 1 {
        let mut out = Vec::new();
        for (index, entry) in entries.iter().copied().enumerate() {
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
                .map(|pc| by_pc.get(pc).cloned().unwrap_or_default())
                .collect();
            out.push((index, lift.emit_function(entry, &texts)));
        }
        out
    } else {
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let lift = &lift;
                    let image = &image;
                    let entries = &entries;
                    scope.spawn(move || {
                        let mut out = Vec::new();
                        let mut index = worker;
                        while index < entries.len() {
                            let entry = entries[index];
                            let body = &lift.funcs[&entry];
                            let items: Vec<(u64, u32)> = body
                                .keys()
                                .filter_map(|pc| image.word(*pc).map(|word| (*pc, word)))
                                .collect();
                            if let Ok((translated, _)) = crate::emitter::translate(&items) {
                                let by_pc: std::collections::HashMap<u64, String> =
                                    items.iter().map(|(pc, _)| *pc).zip(translated).collect();
                                let texts: Vec<String> = body
                                    .keys()
                                    .map(|pc| by_pc.get(pc).cloned().unwrap_or_default())
                                    .collect();
                                out.push((index, lift.emit_function(entry, &texts)));
                            }
                            index += workers;
                        }
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect()
        })
    };
    emitted.sort_by_key(|(index, _)| *index);
    let mut used = std::collections::HashSet::new();
    for (_, text) in &emitted {
        collect_fpx(text, &mut used);
    }
    std::fs::write(out.join("a64_fpx.h"), crate::text::fpx_header(&used))
        .map_err(|e| e.to_string())?;
    let written = entries.len();
    let mut written_entries = entries.clone();
    written_entries.extend(lift.hooks.keys().copied());
    written_entries.sort_unstable();
    written_entries.dedup();
    let reference = regex::Regex::new(r"\bf_([0-9a-f]{8})\b").unwrap();
    let header = |decls: &std::collections::BTreeSet<u64>| {
        let mut text = String::from("#include \"a64.h\"\n");
        for entry in decls {
            text.push_str(&format!("void f_{entry:08x}(Ctx *A64_NOESC);\n"));
        }
        text
    };
    let mut chunk = String::new();
    let mut decls = std::collections::BTreeSet::new();
    let mut size = 0usize;
    let mut file = 0usize;
    let mut files = Vec::new();
    for (_, text) in emitted {
        for caps in reference.captures_iter(&text) {
            if let Ok(entry) = u64::from_str_radix(&caps[1], 16) {
                decls.insert(entry);
            }
        }
        chunk.push_str(&text);
        size += text.len();
        if size >= chunk_bytes {
            let path = out.join(format!("code_{file:03}.c"));
            let body = format!("{}{}", header(&decls), chunk);
            std::fs::write(&path, &body).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push(path.to_string_lossy().into_owned());
            file += 1;
            chunk.clear();
            decls.clear();
            size = 0;
        }
    }
    if !chunk.is_empty() {
        let path = out.join(format!("code_{file:03}.c"));
        let body = format!("{}{}", header(&decls), chunk);
        std::fs::write(&path, &body).map_err(|e| format!("{}: {e}", path.display()))?;
        files.push(path.to_string_lossy().into_owned());
    }
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
    Ok(serde_json::json!({"files": files, "functions": written, "hooks": lift.hooks.len()}))
}
pub fn emit_json(exefs: &str, entry: u64) -> Result<serde_json::Value, String> {
    let image = crate::image::Image::build(std::path::Path::new(exefs), 0, false, &[], &[])?;
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
        .map(|pc| by_pc.get(pc).cloned().unwrap_or_default())
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
        serde_json::json!({"entry": format!("{entry:x}"), "frame_false": frame_false, "v_ops": v_ops, "branches": branches, "c": lift.emit_function(entry, &texts)}),
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
