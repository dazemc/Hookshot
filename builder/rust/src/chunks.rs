//! Stable chunk boundaries and giant-chunk splitting for the lifted C.
//! Ported from nx2/chunks.py (the parts that do not need the lifter).
#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Value, json};

use crate::lifted_c;

pub const FILE: &str = "chunks.txt";
pub const COSTS: &str = "chunk_costs.txt";
pub const QUAD: u64 = 350_000;
pub const CHUNK_BYTES: u64 = 24 << 20;

pub fn fn_cost(size: u64) -> u64 {
    size + size * size / QUAD
}

pub fn text_cost(text: &str) -> u64 {
    let spans = lifted_c::spans_noted(text);
    let Some(first) = spans.first() else { return 0 };
    first.start as u64
        + spans
            .iter()
            .map(|span| fn_cost((span.end - span.start) as u64))
            .sum::<u64>()
}

pub fn load(image_dir: &Path) -> Vec<u64> {
    fs::read_to_string(image_dir.join(FILE))
        .map(|text| {
            text.lines()
                .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
                .filter_map(|line| u64::from_str_radix(line.trim(), 16).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(image_dir: &Path, starts: &[u64]) -> Result<(), String> {
    fs::create_dir_all(image_dir).map_err(|e| format!("{}: {e}", image_dir.display()))?;
    let mut text = String::from(
        "# first guest address of each lifted C chunk after the first (nx2/chunks.py)\n",
    );
    let mut sorted = starts.to_vec();
    sorted.sort_unstable();
    for start in sorted {
        text.push_str(&format!("{start:08x}\n"));
    }
    fs::write(image_dir.join(FILE), text)
        .map_err(|e| format!("{}: {e}", image_dir.join(FILE).display()))
}

/// The boundaries of an existing size-chunked lift: the first function in
/// each code_NNN.c after the first.
pub fn seed_from_gen(folder: &Path) -> Vec<u64> {
    let mut starts = Vec::new();
    let mut n = 1;
    loop {
        let path = folder.join(format!("code_{n:03}.c"));
        let Ok(text) = fs::read_to_string(&path) else {
            break;
        };
        if let Some(span) = lifted_c::spans(&text).first() {
            starts.push(u64::from_str_radix(span.address, 16).unwrap_or(0));
        }
        n += 1;
    }
    starts
}

fn read_costs(folder: &Path) -> HashMap<String, u64> {
    let mut known = HashMap::new();
    if let Ok(text) = fs::read_to_string(folder.join(COSTS)) {
        for line in text.lines() {
            let mut parts = line.splitn(2, ' ');
            let (Some(name), Some(value)) = (
                parts.next(),
                parts
                    .next()
                    .and_then(|value| value.trim().parse::<u64>().ok()),
            ) else {
                continue;
            };
            if folder.join(name).exists() {
                known.insert(name.to_owned(), value);
            }
        }
    }
    known
}

pub fn write_costs(folder: &Path, costs: &HashMap<String, u64>) -> Result<(), String> {
    let mut known = read_costs(folder);
    known.extend(costs.iter().map(|(name, cost)| (name.clone(), *cost)));
    let mut lines: Vec<String> = known
        .into_iter()
        .map(|(name, cost)| format!("{name} {cost}"))
        .collect();
    lines.sort();
    fs::write(folder.join(COSTS), lines.join("\n") + "\n")
        .map_err(|e| format!("{}: {e}", folder.join(COSTS).display()))
}

fn whole_write(path: &Path, text: &str) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".whole");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Cut chunks whose estimated compile cost is well past a plain chunk into
/// parts of about equal cost, at function starts. The first part keeps the
/// name. Returns the new part paths.
pub fn split_big(folder: &Path, files: &[PathBuf], budget: u64) -> Result<Vec<PathBuf>, String> {
    let part = Regex::new(r"^code_(\d{3})_(\d+)\.c$").unwrap();
    let chunk = Regex::new(r"^code_(\d{3})\.c$").unwrap();
    let mut made = Vec::new();
    let mut costs = HashMap::new();
    for path in files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(found) = chunk.captures(&name) else {
            continue;
        };
        let number = found.get(1).unwrap().as_str().to_owned();
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let spans = lifted_c::spans_noted(&text);
        let marks: Vec<usize> = spans.iter().map(|span| span.start).collect();
        if marks.is_empty() {
            costs.insert(name, text.len() as u64);
            continue;
        }
        let mut ends = marks[1..].to_vec();
        ends.push(text.len());
        let cost: Vec<u64> = marks
            .iter()
            .zip(&ends)
            .map(|(a, b)| fn_cost((b - a) as u64))
            .collect();
        let total: u64 = cost.iter().sum();
        if total <= budget * 3 / 2 || marks.len() < 2 {
            costs.insert(name, marks[0] as u64 + total);
            continue;
        }
        let parts = total.div_ceil(budget);
        let mut cuts = vec![0usize];
        let mut run = 0u64;
        let mut k = 1u64;
        for (i, c) in cost.iter().copied().enumerate() {
            if i > 0 && run >= total * k / parts {
                cuts.push(i);
                k = run * parts / total + 1;
            }
            run += c;
        }
        let mut bounds: Vec<usize> = cuts.iter().map(|index| marks[*index]).collect();
        bounds.push(text.len());
        let preamble = &text[..marks[0]];
        for j in 0..cuts.len() {
            let out = if j == 0 {
                path.clone()
            } else {
                folder.join(format!("code_{number}_{j}.c"))
            };
            let piece = if j == 0 {
                text[..bounds[1]].to_owned()
            } else {
                format!("{preamble}{}", &text[bounds[j]..bounds[j + 1]])
            };
            whole_write(&out, &piece)?;
            let end = cuts.get(j + 1).copied().unwrap_or(cost.len());
            costs.insert(
                out.file_name().unwrap().to_string_lossy().into_owned(),
                preamble.len() as u64 + cost[cuts[j]..end].iter().sum::<u64>(),
            );
            if j > 0 {
                made.push(out);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if part.is_match(&name)
                && !made.iter().any(|path| {
                    path.file_name()
                        .map(|n| n == entry.file_name())
                        .unwrap_or(false)
                })
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    write_costs(folder, &costs)?;
    Ok(made)
}

pub fn split_big_json(folder: &str, files: &[String], budget: u64) -> Result<Value, String> {
    let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    let made = split_big(
        Path::new(folder),
        &paths,
        if budget == 0 { CHUNK_BYTES } else { budget },
    )?;
    Ok(json!({"made": made.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_grow_with_the_square_of_a_function() {
        assert_eq!(fn_cost(1000), 1002);
        assert!(fn_cost(QUAD) == 2 * QUAD);
    }

    #[test]
    fn splits_a_giant_chunk_and_keeps_the_preamble() {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-chunks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let mut text = String::from("/* preamble */\n");
        for i in 0..6 {
            text.push_str(&format!(
                "/* note {i} */\nvoid f_8000000{i}(Ctx *__restrict A64_NOESC c) {{\n"
            ));
            text.push_str(&"x".repeat(200_000));
            text.push_str("\n}\n");
        }
        let path = folder.join("code_000.c");
        fs::write(&path, &text).unwrap();
        let made = split_big(&folder, &[path], 300_000).unwrap();
        assert!(!made.is_empty());
        assert!(folder.join("code_000_1.c").is_file());
        assert!(
            fs::read_to_string(folder.join("code_000_1.c"))
                .unwrap()
                .starts_with("/* preamble */")
        );
        assert!(
            fs::read_to_string(folder.join("chunk_costs.txt"))
                .unwrap()
                .contains("code_000.c")
        );
        let _ = fs::remove_dir_all(folder);
    }
}
