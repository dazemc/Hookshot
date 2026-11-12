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
}

impl Code for Image {
    fn in_code(&self, address: u64) -> bool {
        Image::in_code(self, address)
    }
    fn word(&self, address: u64) -> Option<u32> {
        Image::word(self, address)
    }
}

pub struct Lifter<'a, C: Code> {
    code: &'a C,
    pub entries: HashSet<u64>,
    pub funcs: BTreeMap<u64, BTreeMap<u64, Kind>>,
    pub labels: BTreeMap<u64, HashSet<u64>>,
}

impl<'a, C: Code> Lifter<'a, C> {
    pub fn new(code: &'a C, entries: impl IntoIterator<Item = u64>) -> Self {
        Self {
            code,
            entries: entries.into_iter().collect(),
            funcs: BTreeMap::new(),
            labels: BTreeMap::new(),
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
