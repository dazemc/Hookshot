//! Function discovery over the guest image: walk from every entry until a
//! terminator or another entry. Ported from a64_lift.Lifter.walk; jump-table
//! resolution and C emission follow in later steps.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashSet};

use crate::a64::{self, Kind};
use crate::image::Image;

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
