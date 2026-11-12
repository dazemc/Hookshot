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
/// The estimated compile cost one compile unit is cut to (nx2/build.py's
/// UNIT_BYTES): a 25 MB chunk at -O1 takes ~1.4 GB of the compiler and 70-95 s,
/// six such pieces take ~0.45 GB each and a sixth of that time, so the build
/// runs more of them at once and one changed function recompiles one piece.
pub const UNIT_BYTES: u64 = 6 << 20;
/// In the units folder: each chunk's size, time and pieces, so an unchanged
/// chunk is not read again (an update build splits in milliseconds).
pub const MANIFEST: &str = ".units.json";

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

/// Whether a lifted chunk is split into compile units: the numbered chunks
/// (`code_000.c`, and a giant chunk's `code_000_1.c`). A hot chunk is left
/// alone: split_hot has made its pieces already.
pub fn is_compile_unit_source(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".c") else {
        return false;
    };
    let Some(rest) = stem.strip_prefix("code_") else {
        return false;
    };
    let mut parts = rest.split('_');
    let number = parts.next().unwrap_or_default();
    let part = parts.next();
    parts.next().is_none()
        && number.len() == 3
        && number.bytes().all(|b| b.is_ascii_digit())
        && part.is_none_or(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// The compile unit holding piece `index` of `name`: the chunk itself for the
/// first, `code_000_u1.c` and on for the rest.
pub fn unit_name(name: &str, index: usize) -> String {
    if index == 0 {
        return name.to_owned();
    }
    let stem = name.strip_suffix(".c").unwrap_or(name);
    format!("{stem}_u{index}.c")
}

/// One lifted chunk cut at function boundaries into compile units: the whole
/// text when it is small enough, else equal-cost runs of whole functions, each
/// with the chunk's preamble at its head. The functions are partitioned, not
/// copied, so no definition appears in two units. Returns each unit's text and
/// its estimated cost (what LPT ordering and the countdown need).
pub fn unit_texts(text: &str, budget: u64) -> Vec<(String, u64)> {
    let spans = lifted_c::spans_noted(text);
    if spans.len() < 2 {
        return vec![(text.to_owned(), text.len() as u64)];
    }
    let first = spans[0].start;
    let cost: Vec<u64> = spans
        .iter()
        .map(|span| fn_cost((span.end - span.start) as u64))
        .collect();
    let total: u64 = cost.iter().sum();
    if total <= budget * 3 / 2 {
        return vec![(text.to_owned(), first as u64 + total)];
    }
    let parts = total.div_ceil(budget).min(spans.len() as u64).max(2);
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
    let mut bounds: Vec<usize> = cuts.iter().map(|index| spans[*index].start).collect();
    bounds.push(text.len());
    let preamble = &text[..first];
    let mut out = Vec::with_capacity(cuts.len());
    for (j, cut) in cuts.iter().copied().enumerate() {
        let piece = if j == 0 {
            text[..bounds[1]].to_owned()
        } else {
            format!("{preamble}{}", &text[bounds[j]..bounds[j + 1]])
        };
        let end = cuts.get(j + 1).copied().unwrap_or(cost.len());
        out.push((piece, first as u64 + cost[cut..end].iter().sum::<u64>()));
    }
    out
}

/// Write the compile units of every numbered chunk in `game_gen` to `units`,
/// leaving the lifted C itself as the lift wrote it. A chunk whose size and
/// time are as the last run left them is not read again, and a unit whose text
/// is already there is not written again, so an update build only touches the
/// pieces whose functions changed. Stale pieces are removed here; their object
/// files are the driver's to forget. Returns the units to compile, and their
/// estimated costs.
pub fn write_units(
    game_gen: &Path,
    units: &Path,
    budget: u64,
) -> Result<(Vec<PathBuf>, HashMap<String, u64>), String> {
    fs::create_dir_all(units).map_err(|e| format!("{}: {e}", units.display()))?;
    let manifest = units.join(MANIFEST);
    let previous: HashMap<String, Value> = fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("chunks").and_then(Value::as_object).cloned())
        .map(|chunks| chunks.into_iter().collect())
        .unwrap_or_default();
    let mut made = Vec::new();
    let mut costs = HashMap::new();
    let mut current = serde_json::Map::new();
    let mut keep = std::collections::HashSet::new();
    let mut names: Vec<String> = fs::read_dir(game_gen)
        .map_err(|e| format!("{}: {e}", game_gen.display()))?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            is_compile_unit_source(&name).then_some(name)
        })
        .collect();
    names.sort();
    for name in names {
        let path = game_gen.join(&name);
        let meta = fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let stamp = [
            meta.len(),
            meta.modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|since| since.as_nanos() as u64)
                .unwrap_or(0),
        ];
        let old = previous
            .get(&name)
            .and_then(Value::as_array)
            .filter(|old| old.len() == 4);
        let reusable = old.is_some_and(|old| {
            old[0].as_u64() == Some(stamp[0])
                && old[1].as_u64() == Some(stamp[1])
                && old[2]
                    .as_array()
                    .is_some_and(|pieces| {
                        pieces
                            .iter()
                            .filter_map(Value::as_str)
                            .all(|piece| units.join(piece).is_file())
                    })
        });
        if reusable {
            // (the lift and its patches left this chunk as it was: no need to
            // read 25 MB to say the pieces are still the same)
            let old = old.unwrap();
            let pieces = old[2].as_array().unwrap();
            let piece_costs = old[3].as_array().unwrap();
            for (piece, cost) in pieces.iter().zip(piece_costs) {
                let piece = piece.as_str().unwrap_or_default().to_owned();
                made.push(units.join(&piece));
                costs.insert(piece.clone(), cost.as_u64().unwrap_or(0));
                keep.insert(piece);
            }
            current.insert(name, Value::Array(old.clone()));
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut piece_names = Vec::new();
        let mut piece_costs = Vec::new();
        for (index, (piece, cost)) in unit_texts(&text, budget).into_iter().enumerate() {
            let out_name = unit_name(&name, index);
            let out = units.join(&out_name);
            costs.insert(out_name.clone(), cost);
            piece_names.push(out_name.clone());
            piece_costs.push(json!(cost));
            keep.insert(out_name.clone());
            let current_text = fs::read_to_string(&out).ok();
            if current_text.as_deref() != Some(piece.as_str()) {
                whole_write(&out, &piece)?;
            }
            made.push(out);
        }
        current.insert(
            name,
            json!([stamp[0], stamp[1], piece_names, piece_costs]),
        );
    }
    // (an earlier build's pieces: a chunk that changed its cuts, or a game
    // update with fewer chunks. Their objects are the driver's to forget.)
    if let Ok(entries) = fs::read_dir(units) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".c") && !keep.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let updated = Value::Object(current);
    let old = Value::Object(previous.into_iter().collect());
    if updated != old {
        whole_write(
            &manifest,
            &serde_json::to_string(&json!({"chunks": updated})).map_err(|e| e.to_string())?,
        )?;
    }
    Ok((made, costs))
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

    /// `count` functions with a note, each `size` bytes of body.
    fn lifted_chunk(count: usize, size: usize) -> String {
        let mut text = String::from("#include \"a64.h\"\nvoid f_80000000(Ctx *A64_NOESC);\n");
        for i in 0..count {
            text.push_str(&format!(
                "/* note {i} */\nvoid f_{:08x}(Ctx *__restrict A64_NOESC c) {{\n",
                0x80010000 + i * 16
            ));
            text.push_str(&"x".repeat(size));
            text.push_str("\n}\n");
        }
        text
    }

    #[test]
    fn compile_units_partition_the_functions() {
        let text = lifted_chunk(24, 100_000);
        let units = unit_texts(&text, 300_000);
        assert!(units.len() > 2, "{} units", units.len());
        let preamble = &text[..lifted_c::spans_noted(&text)[0].start];
        for (piece, cost) in &units {
            assert!(*cost > 0);
            assert!(piece.starts_with(preamble));
        }
        // (every definition exactly once, in the same order, and nothing else
        // copied: the units' functions concatenated are the chunk's functions)
        let bodies = |t: &str| -> String {
            let spans = lifted_c::spans_noted(t);
            let first = spans.first().map(|span| span.start).unwrap_or(t.len());
            t[first..].to_owned()
        };
        let joined: String = units.iter().map(|(piece, _)| bodies(piece)).collect();
        assert_eq!(joined, bodies(&text));
        // (the preamble is the only text a unit repeats: declarations, no bodies)
        let preamble = &text[..lifted_c::spans_noted(&text)[0].start];
        for (piece, _) in &units {
            assert!(piece.starts_with(preamble));
        }
    }

    #[test]
    fn a_chunk_under_the_budget_is_one_unit() {
        let text = lifted_chunk(4, 1000);
        let units = unit_texts(&text, 6 << 20);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].0, text);
        assert!(units[0].1 >= text.len() as u64);
    }

    #[test]
    fn one_changed_function_changes_one_unit() {
        let text = lifted_chunk(24, 100_000);
        let mut changed = text.clone();
        let at = changed.find("/* note 11 */").unwrap();
        let body = changed[at..].find("xxxx").unwrap() + at;
        changed.replace_range(body..body + 4, "yyyY"); // (same length: the cuts do not move)
        let before = unit_texts(&text, 300_000);
        let after = unit_texts(&changed, 300_000);
        assert_eq!(before.len(), after.len());
        let differing = before
            .iter()
            .zip(&after)
            .filter(|((a, _), (b, _))| a != b)
            .count();
        assert_eq!(differing, 1, "{differing} units changed");
    }

    #[test]
    fn units_are_named_after_their_chunk() {
        assert_eq!(unit_name("code_000.c", 0), "code_000.c");
        assert_eq!(unit_name("code_000.c", 1), "code_000_u1.c");
        assert_eq!(unit_name("code_000_2.c", 3), "code_000_2_u3.c");
        assert!(is_compile_unit_source("code_000.c"));
        assert!(is_compile_unit_source("code_012_3.c"));
        assert!(!is_compile_unit_source("code_hot_0.c"));
        assert!(!is_compile_unit_source("a64_table.c"));
        assert!(!is_compile_unit_source("code_000_u1.c"));
    }

    #[test]
    fn write_units_leaves_the_lift_and_forgets_stale_pieces() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-units-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let game_gen = base.join("gen");
        let units = base.join("units");
        fs::create_dir_all(&game_gen).unwrap();
        fs::create_dir_all(&units).unwrap();
        let text = lifted_chunk(24, 100_000);
        fs::write(game_gen.join("code_000.c"), &text).unwrap();
        fs::write(units.join("code_999_u9.c"), "/* an earlier build's */\n").unwrap();
        let (made, costs) = write_units(&game_gen, &units, 300_000).unwrap();
        assert!(made.len() > 2);
        assert_eq!(
            fs::read_to_string(game_gen.join("code_000.c")).unwrap(),
            text,
            "the lifted C is as the lift wrote it"
        );
        assert!(!units.join("code_999_u9.c").exists());
        assert!(costs.contains_key("code_000_u1.c"));
        // (a second run writes nothing again: the pieces are already there)
        let modified = fs::metadata(units.join("code_000_u1.c"))
            .unwrap()
            .modified()
            .unwrap();
        let (again, _) = write_units(&game_gen, &units, 300_000).unwrap();
        assert_eq!(again.len(), made.len());
        assert_eq!(
            fs::metadata(units.join("code_000_u1.c"))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        let _ = fs::remove_dir_all(base);
    }
}
