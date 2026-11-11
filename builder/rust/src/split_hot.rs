//! code_hot.c in pieces that compile side by side: 83 MB in one file took 79
//! minutes on one core. Ported from nx2/split_hot.py.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::{Value, json};

use crate::lifted_c;

const QUAD: u64 = 350_000;
const COSTS: &str = "chunk_costs.txt";

fn fn_cost(size: u64) -> u64 {
    size + size * size / QUAD
}

fn read_costs(folder: &Path) -> HashMap<String, u64> {
    let mut known = HashMap::new();
    if let Ok(text) = fs::read_to_string(folder.join(COSTS)) {
        for line in text.lines() {
            let mut parts = line.splitn(2, ' ');
            let Some(name) = parts.next() else { continue };
            let Some(value) = parts
                .next()
                .and_then(|value| value.trim().parse::<u64>().ok())
            else {
                continue;
            };
            if folder.join(name).exists() {
                known.insert(name.to_owned(), value);
            }
        }
    }
    known
}

fn write_costs(folder: &Path, costs: &HashMap<String, u64>) -> Result<(), String> {
    let mut known = read_costs(folder);
    known.extend(costs.iter().map(|(key, value)| (key.clone(), *value)));
    let mut lines: Vec<String> = known
        .into_iter()
        .map(|(name, cost)| format!("{name} {cost}"))
        .collect();
    lines.sort();
    fs::write(folder.join(COSTS), lines.join("\n") + "\n")
        .map_err(|e| format!("{}: {e}", folder.join(COSTS).display()))
}

struct Union {
    parent: HashMap<String, String>,
    size: HashMap<String, u64>,
}

impl Union {
    fn root(&mut self, name: &str) -> String {
        let mut at = name.to_owned();
        while self.parent[&at] != at {
            let up = self.parent[&at].clone();
            self.parent.insert(at.clone(), self.parent[&up].clone());
            at = up;
        }
        at
    }
}

/// Split code_hot.c into `pieces` files, grouping functions that call each
/// other and dealing the groups out by compile cost.
pub fn split(folder: &Path, pieces: usize) -> Result<Value, String> {
    let path = folder.join("code_hot.c");
    let whole = folder.join("code_hot.c.whole");
    if path.exists() {
        fs::rename(&path, &whole).map_err(|e| format!("{}: {e}", whole.display()))?;
    }
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("code_hot_") && name.ends_with(".c") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let text = fs::read_to_string(&whole).map_err(|e| format!("{}: {e}", whole.display()))?;
    let spans = lifted_c::spans(&text);
    if spans.is_empty() {
        return Err(format!("split_hot: no functions in {}", whole.display()));
    }
    let preamble = &text[..spans[0].start];
    let bodies: Vec<(&str, &str)> = spans
        .iter()
        .map(|span| (span.address, &text[span.start..span.end]))
        .collect();
    let total: u64 = bodies.iter().map(|(_, body)| body.len() as u64).sum();
    let limit = total as f64 * 1.1 / pieces as f64;
    let sizes: HashMap<&str, u64> = bodies
        .iter()
        .map(|(name, body)| (*name, body.len() as u64))
        .collect();
    let call = Regex::new(r"\bf_([0-9a-f]{8})\(c\)").unwrap();
    let mut edges: HashMap<(&str, &str), u64> = HashMap::new();
    for (name, body) in &bodies {
        for found in call.captures_iter(body) {
            let callee = found.get(1).unwrap().as_str();
            if callee != *name && sizes.contains_key(callee) {
                *edges.entry((*name, callee)).or_default() += 1;
            }
        }
    }
    let mut ordered: Vec<((&str, &str), u64)> = edges.into_iter().collect();
    ordered.sort_by(|a, b| {
        let left = sizes[a.0.0] as f64 / a.1 as f64;
        let right = sizes[b.0.0] as f64 / b.1 as f64;
        left.partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    let mut union = Union {
        parent: bodies
            .iter()
            .map(|(name, _)| ((*name).to_owned(), (*name).to_owned()))
            .collect(),
        size: sizes
            .iter()
            .map(|(name, size)| ((*name).to_owned(), *size))
            .collect(),
    };
    for ((from, to), _) in ordered {
        let a = union.root(from);
        let b = union.root(to);
        if a != b && union.size[&a] + union.size[&b] <= limit as u64 {
            union.parent.insert(b.clone(), a.clone());
            let size = union.size[&a] + union.size[&b];
            union.size.insert(a, size);
        }
    }
    let mut groups: Vec<Vec<&str>> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (name, _) in &bodies {
        let root = union.root(name);
        let at = *index.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[at].push(name);
    }
    let cost: HashMap<&str, u64> = bodies
        .iter()
        .map(|(name, body)| (*name, fn_cost(body.len() as u64)))
        .collect();
    groups.sort_by(|a, b| {
        let left: u64 = a.iter().map(|name| cost[name]).sum();
        let right: u64 = b.iter().map(|name| cost[name]).sum();
        right.cmp(&left).then(a.iter().min().cmp(&b.iter().min()))
    });
    let mut bins: Vec<(u64, Vec<&str>)> = vec![(0, Vec::new()); pieces.max(1)];
    for group in groups {
        let at = (0..bins.len()).min_by_key(|i| bins[*i].0).unwrap();
        bins[at].0 += group.iter().map(|name| cost[name]).sum::<u64>();
        bins[at].1.extend(group);
    }
    let mut costs = HashMap::new();
    let mut written = Vec::new();
    for (i, (cost, mut names)) in bins.into_iter().enumerate() {
        if names.is_empty() {
            continue;
        }
        names.sort();
        let mut out = String::from(preamble);
        for name in &names {
            let body = bodies
                .iter()
                .find(|(address, _)| address == name)
                .map(|(_, body)| *body)
                .unwrap();
            out.push_str(body);
        }
        let file = format!("code_hot_{i}.c");
        fs::write(folder.join(&file), out)
            .map_err(|e| format!("{}: {e}", folder.join(&file).display()))?;
        costs.insert(file.clone(), preamble.len() as u64 + cost);
        written.push(file);
    }
    write_costs(folder, &costs)?;
    let kept = ordered_kept(&bodies, &mut union, &call);
    Ok(json!({
        "pieces": written,
        "costs": costs,
        "calls_kept": kept,
        "functions": bodies.len(),
    }))
}

fn ordered_kept(bodies: &[(&str, &str)], union: &mut Union, call: &Regex) -> u64 {
    let mut kept = 0;
    for (name, body) in bodies {
        for found in call.captures_iter(body) {
            let callee = found.get(1).unwrap().as_str();
            if callee != *name
                && union.parent.contains_key(callee)
                && union.root(name) == union.root(callee)
            {
                kept += 1;
            }
        }
    }
    kept
}

pub fn split_json(folder: &str, pieces: usize) -> Result<Value, String> {
    split(Path::new(folder), pieces)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_hot_file_into_pieces() {
        let folder = std::env::temp_dir().join(format!("hookshot-rust-hot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let mut text = String::from("/* preamble */\n");
        for (i, callee) in [None, Some("80000000"), Some("80000000"), None]
            .iter()
            .enumerate()
        {
            text.push_str(&format!(
                "void f_8000000{i}(Ctx *__restrict A64_NOESC c) {{\n"
            ));
            if let Some(callee) = callee {
                text.push_str(&format!("  f_{callee}(c);\n"));
            }
            text.push_str("}\n");
        }
        fs::write(folder.join("code_hot.c"), &text).unwrap();
        fs::write(folder.join("code_000.c"), "void f_80000009(void) {}\n").unwrap();
        fs::write(folder.join("chunk_costs.txt"), "code_000.c 42\n").unwrap();
        let answer = split(&folder, 2).unwrap();
        assert_eq!(answer["functions"], 4);
        assert!(folder.join("code_hot_0.c").is_file());
        assert!(folder.join("code_hot_1.c").is_file());
        assert!(folder.join("code_hot.c.whole").is_file());
        let costs = fs::read_to_string(folder.join("chunk_costs.txt")).unwrap();
        assert!(costs.contains("code_000.c 42"));
        assert!(costs.contains("code_hot_0.c"));
        let piece = fs::read_to_string(folder.join("code_hot_0.c")).unwrap();
        assert!(piece.starts_with("/* preamble */"));
        let _ = fs::remove_dir_all(folder);
    }
}
