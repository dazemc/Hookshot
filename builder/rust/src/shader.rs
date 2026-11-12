//! Every shader program in the game's archives, for the precompile before
//! the first start. Ported from nx2/shader_census.py.
//!
//! Output: "HKSEED01", u32 programs, u32 stages, then each stage once
//! (u64 key, u32 stage, u32 control bytes, u32 code bytes, control, code),
//! then each program (u32 count, u32 stage indices).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha1::{Digest, Sha1};

const CONTROL_MAGIC: &[u8; 4] = b"\x34\x12\x76\x98";
const ZSTD: &[u8; 4] = b"\x28\xb5\x2f\xfd";
const SLOTS: [usize; 9] = [8, 16, 24, 32, 40, 48, 56, 64, 72];

type Stage = (u16, Vec<u8>, Vec<u8>);

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    data.get(at..at + 2)
        .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn zstd_decode(data: &[u8], dictionary: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if let Some(dictionary) = dictionary {
        let mut decoder = zstd::stream::read::Decoder::with_dictionary(data, dictionary)
            .map_err(|e| e.to_string())?;
        decoder.read_to_end(&mut out).map_err(|e| e.to_string())?;
    } else {
        let mut decoder = zstd::stream::read::Decoder::new(data).map_err(|e| e.to_string())?;
        decoder.read_to_end(&mut out).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn unpack(data: &[u8], dictionaries: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    if !data.starts_with(ZSTD) {
        return Ok(data.to_vec());
    }
    if let Ok(out) = zstd_decode(data, None) {
        return Ok(out);
    }
    for dictionary in dictionaries {
        if let Ok(out) = zstd_decode(data, Some(dictionary)) {
            return Ok(out);
        }
    }
    Err("no dictionary fits".into())
}

fn dictionaries(romfs: &Path) -> Result<Vec<Vec<u8>>, String> {
    let path = romfs.join("Pack/ZsDic.pack.zs");
    let raw = zstd_decode(
        &fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        None,
    )?;
    let start = u32_at(&raw, 0x0c).ok_or("short ZsDic")? as usize;
    let count = u16_at(&raw, 0x1a).ok_or("short ZsDic")? as usize;
    let mut out = Vec::new();
    for k in 0..count {
        let at = 0x20 + k * 16;
        let st = u32_at(&raw, at + 8).ok_or("short ZsDic")? as usize;
        let end = u32_at(&raw, at + 12).ok_or("short ZsDic")? as usize;
        out.push(
            raw.get(start + st..start + end)
                .ok_or("ZsDic entry outside the file")?
                .to_vec(),
        );
    }
    Ok(out)
}

fn control_at(data: &[u8], at: usize) -> Option<(Vec<u8>, usize)> {
    let control = data.get(at..at + 0xd8)?;
    if !control.starts_with(CONTROL_MAGIC) || u16_at(control, 0x56)? > 7 {
        return None;
    }
    Some((control.to_vec(), u32_at(control, 0x44)? as usize))
}

fn stage(control: Vec<u8>, code: Vec<u8>) -> Stage {
    (u16_at(&control, 0x56).unwrap_or(0), control, code)
}

fn bnsh_programs(data: &[u8]) -> Result<Vec<Vec<Stage>>, String> {
    if !data.starts_with(b"BNSH")
        || data.get(12..14) != Some(b"\xff\xfe")
        || data.get(96..100) != Some(b"grsc")
    {
        return Err("unsupported BNSH header".into());
    }
    let size = u32_at(data, 28).ok_or("short BNSH")? as usize;
    let data = data.get(..size).ok_or("BNSH size outside the file")?;
    let count = u32_at(data, 124).ok_or("short BNSH")? as usize;
    let variations = u64_at(data, 128).ok_or("short BNSH")? as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let program = u64_at(data, variations + i * 64 + 16).ok_or("short BNSH")? as usize;
        if program == 0 || program + 80 > size {
            continue;
        }
        let mut stages = Vec::new();
        for rel in SLOTS {
            let Some(offset) = u64_at(data, program + rel).map(|offset| offset as usize) else {
                continue;
            };
            if offset == 0 || offset + 32 > size {
                continue;
            }
            let Some(control_off) = u64_at(data, offset + 8).map(|value| value as usize) else {
                continue;
            };
            let Some(code_off) = u64_at(data, offset + 16).map(|value| value as usize) else {
                continue;
            };
            let Some(code_size) = u32_at(data, offset + 24).map(|value| value as usize) else {
                continue;
            };
            let Some(control_size) = u32_at(data, offset + 28).map(|value| value as usize) else {
                continue;
            };
            if !(0x60..=0x1000).contains(&control_size)
                || control_off + control_size > size
                || code_off + code_size > size
            {
                continue;
            }
            let control = &data[control_off..control_off + control_size];
            if !control.starts_with(CONTROL_MAGIC) {
                continue;
            }
            stages.push(stage(
                control.to_vec(),
                data[code_off..code_off + code_size].to_vec(),
            ));
        }
        if !stages.is_empty() {
            out.push(stages);
        }
    }
    Ok(out)
}

fn bahs_programs(data: &[u8]) -> Result<Vec<Vec<Stage>>, String> {
    let base = u32_at(data, 0x14).ok_or("short BAHS")? as usize;
    let blob = u32_at(data, 0x1c).ok_or("short BAHS")? as usize;
    let table = u32_at(data, base + 0x150).ok_or("short BAHS")? as usize;
    let count = u32_at(data, base + 0x154).ok_or("short BAHS")? as usize;
    let mut at = base + 0x158;
    let mut stages = Vec::new();
    for _ in 0..count {
        let step = u32_at(data, at).ok_or("short BAHS")? as usize;
        if step < 32 {
            return Err(format!("stage at {at:#x}"));
        }
        let offset = u64_at(data, at + 16).ok_or("short BAHS")? as usize;
        let rel = u32_at(data, at + 28).ok_or("short BAHS")? as i32 as isize;
        let (control, end) = control_at(data, (at as isize + 16 + rel) as usize)
            .ok_or_else(|| format!("stage at {at:#x}"))?;
        if blob + offset + end > data.len() {
            return Err(format!("stage at {at:#x}"));
        }
        stages.push(stage(
            control,
            data[blob + offset..blob + offset + end].to_vec(),
        ));
        at += step;
    }
    let table = table + base + 0x150;
    let mut at = table + 8;
    let programs = u32_at(data, table + 4).ok_or("short BAHS")? as usize;
    let mut heads = Vec::new();
    for _ in 0..programs {
        let size = u32_at(data, at).ok_or("short BAHS")? as usize;
        let kinds = u32_at(data, at + 8).ok_or("short BAHS")?;
        let first = u32_at(data, at + 12).ok_or("short BAHS")? as usize;
        heads.push((first, kinds.count_ones() as usize));
        at += size;
    }
    heads.sort();
    let mut out = Vec::new();
    for (i, (first, width)) in heads.iter().copied().enumerate() {
        let last = heads.get(i + 1).map(|(first, _)| *first).unwrap_or(count);
        let width = width.max(1);
        let mut v = first;
        while v < last {
            let end = (v + width).min(stages.len());
            if v < end {
                out.push(stages[v..end].to_vec());
            }
            v += width;
        }
    }
    Ok(out)
}

fn ndsr_programs(data: &[u8], base: usize) -> Result<Vec<Vec<Stage>>, String> {
    let count = u32_at(data, base + 0x0c).ok_or("short NDSR")? as usize;
    let mut items = Vec::new();
    for i in 0..count {
        let at = base + 0x14 + 8 * i;
        items.push((
            u32_at(data, at).ok_or("short NDSR")? as usize,
            u32_at(data, at + 4).ok_or("short NDSR")?,
        ));
    }
    let mut out = Vec::new();
    let mut graphics: Vec<Stage> = Vec::new();
    for (i, (c, ct)) in items.iter().copied().enumerate() {
        let Some((x, xt)) = items.get(i + 1).copied() else {
            break;
        };
        let kind = ct & 15;
        if !matches!(kind, 0 | 3 | 6) || xt & 15 != kind + 1 {
            continue;
        }
        let Some((control, end)) = control_at(data, base + c) else {
            continue;
        };
        if base + x + end > data.len() {
            continue;
        }
        let s = stage(control, data[base + x..base + x + end].to_vec());
        if kind == 6 {
            out.push(vec![s]);
        } else if kind == 0 {
            if !graphics.is_empty() {
                out.push(std::mem::take(&mut graphics));
            }
            graphics.push(s);
        } else if !graphics.is_empty() {
            graphics.push(s);
        }
    }
    if !graphics.is_empty() {
        out.push(graphics);
    }
    Ok(out)
}

fn ndve_programs(data: &[u8]) -> Vec<Vec<Stage>> {
    let mut out = Vec::new();
    if data.get(0x28..0x2c) == Some(b"NDSR") {
        if let Ok(programs) = ndsr_programs(data, 0x28) {
            out.extend(programs);
        }
    }
    let mut at = 0;
    while let Some(found) = find(&data[at..], CONTROL_MAGIC) {
        let c = at + found;
        at = c + 1;
        let Some((control, end)) = control_at(data, c) else {
            continue;
        };
        let pair = (end as u32)
            .to_le_bytes()
            .into_iter()
            .chain(0u32.to_le_bytes())
            .collect::<Vec<_>>();
        let mut search = 0;
        while let Some(found) = find(&data[search..], &pair) {
            let pair_at = search + found;
            search = pair_at + 1;
            if pair_at < 8 {
                continue;
            }
            let entry = pair_at - 8;
            if entry % 4 != 0 {
                continue;
            }
            let Some(to_control) = u32_at(data, entry) else {
                continue;
            };
            let Some(to_code) = u32_at(data, entry + 4) else {
                continue;
            };
            if to_control == 0 {
                continue;
            }
            let table = c as i64 - to_control as i64;
            if table < 0 || table % 4 != 0 || c + to_code as usize + end > data.len() {
                continue;
            }
            let table = table as usize;
            let Some(n) = u32_at(data, table) else {
                continue;
            };
            let Some(blobs) = u32_at(data, table + 4) else {
                continue;
            };
            let first = table + 8 + 8 * blobs as usize;
            if n > 0
                && n < 256
                && blobs < 16
                && entry >= first
                && (entry - first) % 16 == 0
                && (entry - first) / 16 < n as usize
            {
                out.push(vec![stage(
                    control,
                    data[c + to_code as usize..c + to_code as usize + end].to_vec(),
                )]);
                break;
            }
        }
    }
    out
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len())
        .position(|window| window == needle)
}

fn bnsh_all(data: &[u8]) -> Vec<Vec<Stage>> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = find(&data[at..], b"BNSH") {
        let start = at + found;
        at = start + 1;
        if data.get(start + 96..start + 100) == Some(b"grsc") {
            if let Some(size) = u32_at(data, start + 28).map(|size| size as usize) {
                if let Some(body) = data.get(start..start + size) {
                    if let Ok(programs) = bnsh_programs(body) {
                        out.extend(programs);
                    }
                }
            }
        }
    }
    out
}

fn programs_in(data: &[u8]) -> (&'static str, Result<Vec<Vec<Stage>>, String>) {
    if data.starts_with(b"BAHS") {
        ("bahs", bahs_programs(data))
    } else if data.starts_with(b"NDSR") {
        ("ndsr", ndsr_programs(data, 0))
    } else if data.starts_with(b"NDVE") {
        ("ndve", Ok(ndve_programs(data)))
    } else {
        ("bnsh", Ok(bnsh_all(data)))
    }
}

fn sarc_files(data: &[u8]) -> Vec<&[u8]> {
    let Some(start) = u32_at(data, 0x0c).map(|value| value as usize) else {
        return Vec::new();
    };
    let Some(count) = u16_at(data, 0x1a).map(|value| value as usize) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for k in 0..count {
        let at = 0x20 + k * 16;
        let (Some(st), Some(end)) = (u32_at(data, at + 8), u32_at(data, at + 12)) else {
            continue;
        };
        if let Some(file) = data.get(start + st as usize..start + end as usize) {
            out.push(file);
        }
    }
    out
}

fn blobs<'a>(data: &'a [u8], dictionaries: &[Vec<u8>]) -> Vec<Vec<u8>> {
    if data.starts_with(b"SARC") {
        let mut out = Vec::new();
        for inner in sarc_files(data) {
            if let Ok(inner) = unpack(inner, dictionaries) {
                out.extend(blobs(&inner, dictionaries));
            }
        }
        out
    } else {
        vec![data.to_vec()]
    }
}

fn archive_files(romfs: &Path) -> Vec<PathBuf> {
    fn walk(folder: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                dirs.push(entry.path());
            } else if name.ends_with(".zs")
                || name.contains(".bfsha")
                || name.contains(".sarc")
                || name.contains(".pck")
            {
                files.push(entry.path());
            }
        }
        dirs.sort();
        files.sort();
        out.extend(files);
        for dir in dirs {
            walk(&dir, out);
        }
    }
    let mut out = Vec::new();
    for top in ["Shader", "Layout2Shader", "Lib", "Movie", "Effect"] {
        walk(&romfs.join(top), &mut out);
    }
    out
}

enum Scan {
    Read(String),
    Unpack(String),
    Parsed(Vec<(&'static str, Result<Vec<Vec<Stage>>, String>)>),
}

fn scan_file(path: &Path, dictionaries: &[Vec<u8>]) -> Scan {
    let Ok(raw) = fs::read(path) else {
        return Scan::Read(format!("{}: read", path.display()));
    };
    let Ok(data) = unpack(&raw, dictionaries) else {
        return Scan::Unpack(format!("{}: unpack", path.display()));
    };
    Scan::Parsed(
        blobs(&data, dictionaries)
            .into_iter()
            .map(|blob| programs_in(&blob))
            .collect(),
    )
}

/// The seed file for the back end: every program, every stage once.
pub fn census(romfs: &Path, out: Option<&Path>) -> Result<Value, String> {
    let dictionaries = dictionaries(romfs)?;
    let mut stage_index: HashMap<u64, usize> = HashMap::new();
    let mut stage_list: Vec<(u64, Stage)> = Vec::new();
    let mut programs: Vec<Vec<u32>> = Vec::new();
    let mut seen: HashSet<Vec<u32>> = HashSet::new();
    let mut files = 0usize;
    let mut skipped = 0usize;
    let mut containers = 0usize;
    let mut by_kind: HashMap<&'static str, (usize, usize)> = HashMap::new();
    let mut skipped_files: Vec<String> = Vec::new();
    let paths = archive_files(romfs);
    let jobs = std::env::var("HOOKSHOT_CENSUS_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|count| count.get())
                .unwrap_or(1)
                .min(8)
        })
        .max(1)
        .min(paths.len().max(1));
    // (a few files at a time bound the decompressed data held while a batch
    // is merged; the merge stays in archive order, so the seed is the same)
    let batch = (jobs * 2).max(1);
    for paths in paths.chunks(batch) {
        let scans: Vec<Scan> = if jobs <= 1 {
            paths
                .iter()
                .map(|path| scan_file(path, &dictionaries))
                .collect()
        } else {
            let per = paths.len().div_ceil(jobs);
            std::thread::scope(|scope| -> Result<Vec<Scan>, String> {
                let handles: Vec<_> = paths
                    .chunks(per)
                    .map(|chunk| {
                        let dictionaries = &dictionaries;
                        scope.spawn(move || {
                            chunk
                                .iter()
                                .map(|path| scan_file(path, dictionaries))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                let mut scans = Vec::with_capacity(paths.len());
                for handle in handles {
                    scans.extend(
                        handle
                            .join()
                            .map_err(|_| "a shader worker stopped".to_string())?,
                    );
                }
                Ok(scans)
            })?
        };
        for (at, scan) in scans.into_iter().enumerate() {
            files += 1;
            let path = &paths[at];
            let blobs = match scan {
                Scan::Read(reason) | Scan::Unpack(reason) => {
                    skipped += 1;
                    skipped_files.push(reason);
                    continue;
                }
                Scan::Parsed(blobs) => blobs,
            };
            for (kind, found) in blobs {
                containers += 1;
                let found = match found {
                    Ok(found) => found,
                    Err(error) => {
                        skipped += 1;
                        skipped_files.push(format!("{} {kind}: {error}", path.display()));
                        continue;
                    }
                };
                let entry = by_kind.entry(kind).or_default();
                entry.0 += found.len();
                entry.1 += found.iter().map(Vec::len).sum::<usize>();
                for stages in found {
                    let mut ids = Vec::new();
                    for (kind, control, code) in stages {
                        let end = u32_at(&control, 0x44).unwrap_or(0) as usize;
                        let control = &control[..control.len().min(0xd8)];
                        let code = if end <= code.len() {
                            &code[..end]
                        } else {
                            &code[..]
                        };
                        let mut digest = Sha1::new();
                        digest.update(control);
                        digest.update(code);
                        let hash: [u8; 20] = digest.finalize().into();
                        let key = u64::from_le_bytes(hash[..8].try_into().unwrap());
                        let at = *stage_index.entry(key).or_insert_with(|| {
                            stage_list.push((key, (kind, control.to_vec(), code.to_vec())));
                            stage_list.len() - 1
                        });
                        ids.push(at as u32);
                    }
                    ids.sort_unstable();
                    ids.dedup();
                    if seen.insert(ids.clone()) {
                        programs.push(ids);
                    }
                }
            }
        }
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"HKSEED01");
    bytes.extend_from_slice(&(programs.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(stage_list.len() as u32).to_le_bytes());
    for (key, (kind, control, code)) in &stage_list {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&(*kind as u32).to_le_bytes());
        bytes.extend_from_slice(&(control.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(code.len() as u32).to_le_bytes());
        bytes.extend_from_slice(control);
        bytes.extend_from_slice(code);
    }
    for ids in &programs {
        bytes.extend_from_slice(&(ids.len() as u32).to_le_bytes());
        for id in ids {
            bytes.extend_from_slice(&id.to_le_bytes());
        }
    }
    if let Some(out) = out {
        fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(json!({
        "files": files,
        "containers": containers,
        "skipped": skipped,
        "skipped_files": skipped_files,
        "by_kind": by_kind,
        "programs": programs.len(),
        "stages": stage_list.len(),
        "bytes": bytes.len(),
    }))
}

pub fn census_json(romfs: &str, out: Option<&str>) -> Result<Value, String> {
    census(
        Path::new(romfs),
        out.filter(|path| !path.is_empty()).map(Path::new),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_a_marker() {
        assert_eq!(super::find(b"xxABxx", b"AB"), Some(2));
        assert_eq!(super::find(b"xx", b"AB"), None);
    }
}
