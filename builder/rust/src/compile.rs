//! One compile with the object cache nx2/build.py keeps: an object is
//! rebuilt only when its source, flags or headers changed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha1::{Digest, Sha1};

/// What each source's object key was, by the file's size, modification time
/// and id: every build read and hashed each source again to name its object,
/// the lifted C up to three times. A file whose size, time and id are the ones
/// recorded has the content it had; its key for the same flags and headers is
/// the one worked out then. A file written less than RACY_NS before the record
/// was is read again (as git's "racily clean" index entries). A record that
/// cannot be read is no record.
const HASH_CACHE: &str = "hash_cache.json";
const HASH_SCHEME: u32 = 1;
const RACY_NS: u64 = 2_000_000_000;

/// What a dependency blob starts with when it names the headers a compile's
/// depfile says it read (the back end's renderer deps), rather than a folder
/// tree's whole blob. The key is only sound for the object the depfile was
/// written with, so `compile_one` leaves such an object pending until
/// `rekey_object` names it by its own new depfile.
pub const DEPFILE_DEPS: &[u8] = b"depfile:";
/// The stamp of an object compiled from a depfile key, until the depfile the
/// compile just wrote names it. A build that stops before the re-keying leaves
/// it pending, and the next build compiles it again.
pub const DEPFILE_PENDING: &str = "depfile-pending";

/// The compile commands file, beside the objects in the work folder: every
/// compile this build runs, for clangd and clang-tidy.
pub const COMPDB: &str = "compile_commands.json";

#[derive(Clone, Debug, PartialEq)]
struct FileStat {
    size: u64,
    mtime_ns: u64,
    id: u64,
}

#[derive(Clone, Debug)]
struct HashEntry {
    stat: FileStat,
    sha1: String,
    keys: BTreeMap<String, String>,
}

/// The record for one build files folder's cache.
struct Hashes {
    path: PathBuf,
    written_ns: u64,
    files: BTreeMap<String, HashEntry>,
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn file_stat(path: &Path) -> Option<FileStat> {
    let meta = fs::metadata(path).ok()?;
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_nanos().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0);
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    };
    #[cfg(windows)]
    let id = 0; // (file_index is unstable on Windows; size + mtime + the racy window are the guard)
    #[cfg(not(any(unix, windows)))]
    let id = 0;
    Some(FileStat {
        size: meta.len(),
        mtime_ns,
        id,
    })
}

impl Hashes {
    fn load(cache: &Path) -> Self {
        let path = cache.join(HASH_CACHE);
        let empty = Self {
            path: path.clone(),
            written_ns: 0,
            files: BTreeMap::new(),
        };
        let Ok(text) = fs::read_to_string(&path) else {
            return empty;
        };
        let Ok(data) = serde_json::from_str::<Value>(&text) else {
            return empty;
        };
        if data.get("scheme").and_then(Value::as_u64) != Some(u64::from(HASH_SCHEME)) {
            return empty;
        }
        let Some(written_ns) = data.get("written_ns").and_then(Value::as_u64) else {
            return empty;
        };
        let Some(files) = data.get("files").and_then(Value::as_object) else {
            return empty;
        };
        let mut loaded = BTreeMap::new();
        for (src, entry) in files {
            let Some(stat) = entry.get("stat").and_then(Value::as_array) else {
                continue;
            };
            if stat.len() != 3 {
                continue;
            }
            let (Some(size), Some(mtime_ns), Some(id)) =
                (stat[0].as_u64(), stat[1].as_u64(), stat[2].as_u64())
            else {
                continue;
            };
            let Some(sha1) = entry.get("sha1").and_then(Value::as_str) else {
                continue;
            };
            let Some(keys) = entry.get("keys").and_then(Value::as_object) else {
                continue;
            };
            loaded.insert(
                src.clone(),
                HashEntry {
                    stat: FileStat { size, mtime_ns, id },
                    sha1: sha1.to_owned(),
                    keys: keys
                        .iter()
                        .filter_map(|(part, key)| {
                            key.as_str().map(|key| (part.clone(), key.to_owned()))
                        })
                        .collect(),
                },
            );
        }
        Self {
            path,
            written_ns,
            files: loaded,
        }
    }

    /// The record of the sources this build saw, for the next one. Written
    /// atomically; a failure only costs the next build a read.
    fn save(&mut self) {
        self.written_ns = now_ns();
        let files: serde_json::Map<String, Value> = self
            .files
            .iter()
            .filter(|(src, _)| Path::new(src).exists())
            .map(|(src, entry)| {
                (
                    src.clone(),
                    json!({
                        "stat": [entry.stat.size, entry.stat.mtime_ns, entry.stat.id],
                        "sha1": entry.sha1,
                        "keys": entry.keys,
                    }),
                )
            })
            .collect();
        let data = json!({
            "scheme": HASH_SCHEME,
            "written_ns": self.written_ns,
            "files": files,
        });
        let temporary = self.path.with_extension("json.new");
        let write = || -> std::io::Result<()> {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&temporary, serde_json::to_vec(&data)?)?;
            fs::rename(&temporary, &self.path)
        };
        if write().is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }

    /// The record of `src` while it still describes the file on disk, and the
    /// file's stat.
    fn known(&self, src: &Path) -> (Option<&HashEntry>, Option<FileStat>) {
        let Some(stat) = file_stat(src) else {
            return (None, None);
        };
        let entry = self.files.get(&src.to_string_lossy().into_owned());
        match entry {
            Some(entry)
                if entry.stat == stat
                    && stat.mtime_ns < self.written_ns.saturating_sub(RACY_NS) =>
            {
                (Some(entry), Some(stat))
            }
            _ => (None, Some(stat)),
        }
    }

    /// The record of `src` for the content just read: the one there when it
    /// was made from that same content (its other keys hold), else a new one.
    fn remember(&mut self, src: &Path, stat: FileStat, content: &[u8]) -> &mut HashEntry {
        let digest = format!("{:x}", Sha1::digest(content));
        let name = src.to_string_lossy().into_owned();
        let same = self
            .files
            .get(&name)
            .is_some_and(|entry| entry.stat == stat && entry.sha1 == digest);
        if !same {
            self.files.insert(
                name.clone(),
                HashEntry {
                    stat,
                    sha1: digest,
                    keys: BTreeMap::new(),
                },
            );
        }
        self.files.get_mut(&name).expect("just inserted")
    }

    /// `object_key`, without reading a source this record still knows: the
    /// key worked out from it before, for these flags and headers, is the same.
    fn key_of(
        &mut self,
        source: &Path,
        flags: &[String],
        deps: &[u8],
        root: &Path,
    ) -> Result<String, String> {
        let part = kept_part(flags, deps, root);
        let (entry, stat) = self.known(source);
        if let Some(entry) = entry {
            if let Some(key) = entry.keys.get(&part) {
                return Ok(key.clone());
            }
        }
        let content = fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
        let key = key_from_content(&content, flags, deps, root);
        let stat = stat.unwrap_or(FileStat {
            size: content.len() as u64,
            mtime_ns: 0,
            id: 0,
        });
        self.remember(source, stat, &content)
            .keys
            .insert(part, key.clone());
        Ok(key)
    }
}

/// The flags and headers part of an object key, hashed once per job.
fn kept_part(flags: &[String], deps: &[u8], root: &Path) -> String {
    let mut digest = Sha1::new();
    digest.update(flag_text(flags, root).as_bytes());
    digest.update(deps);
    format!("{:x}", digest.finalize())
}

fn key_from_content(content: &[u8], flags: &[String], deps: &[u8], root: &Path) -> String {
    let mut digest = Sha1::new();
    digest.update(content);
    digest.update(flag_text(flags, root).as_bytes());
    digest.update(deps);
    format!("{:x}", digest.finalize())
}

/// The flags an object key is named by: include folders are left out (their
/// headers are in deps), the builder's folder is <root>, and a sysroot's
/// mount path is <sysroot>. `-MD` and `-MF` are left out too: they only write
/// the depfile beside the object (the renderer deps), and change nothing in
/// it.
pub fn flag_text(flags: &[String], root: &Path) -> String {
    let root = root.to_string_lossy();
    flags
        .iter()
        .filter(|flag| {
            !flag.starts_with("-I") && flag.as_str() != "-MD" && !flag.starts_with("-MF")
        })
        .map(|flag| {
            if flag.starts_with("--sysroot=") {
                "--sysroot=<sysroot>".to_owned()
            } else {
                flag.replace(root.as_ref(), "<root>")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn object_key(
    source: &Path,
    flags: &[String],
    deps: &[u8],
    root: &Path,
) -> Result<String, String> {
    let data = fs::read(source).map_err(|e| format!("{}: {e}", source.display()))?;
    Ok(key_from_content(&data, flags, deps, root))
}

fn append(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn cache_put(object: &Path, key: &str, cache: &Path) -> Result<(), String> {
    fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let target = cache.join(format!("{key}.o"));
    if target.exists() {
        return Ok(());
    }
    if fs::hard_link(object, &target).is_err() {
        fs::copy(object, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}

/// Name an object compiled from a depfile key by the key its own new depfile
/// gives, and cache it under that key. `planned_key` is the key `compile_one`
/// used: the old depfile's, which may lack a header the new source reads, or
/// the folders' whole blob, which covers every header. Only an object stamped
/// with one of those (or left pending) is this build's; another build's is
/// left alone. Returns the new key when it re-keyed the object.
pub fn rekey_object(
    source: &Path,
    object: &Path,
    flags: &[String],
    deps: &[u8],
    root: &Path,
    cache: &Path,
    planned_key: &str,
) -> Result<Option<String>, String> {
    if !object.is_file() {
        return Ok(None);
    }
    let stamp = append(object, ".sha1");
    let Ok(old) = fs::read_to_string(&stamp) else {
        return Ok(None);
    };
    if old != DEPFILE_PENDING && old != planned_key {
        return Ok(None);
    }
    let key = object_key(source, flags, deps, root)?;
    cache_put(object, &key, cache)?;
    if key != old {
        fs::write(&stamp, &key).map_err(|e| format!("{}: {e}", stamp.display()))?;
    }
    Ok(Some(key))
}

/// Start a build's compile commands file over: the stages after the driver
/// add theirs to the same file, and an earlier build's commands do not stay.
pub fn reset_compdb(path: &Path) {
    let _ = fs::remove_file(path);
}

/// `<build>/compile_commands.json`: every compile this build runs, as
/// `compile_one` runs it, for clangd and clang-tidy. Stages write their own
/// jobs into the same file; an entry whose object this build compiles again
/// replaces the old one. A failure to write it is no failed build.
pub fn write_compdb(path: &Path, jobs: &[Value]) -> Result<(), String> {
    let directory = std::env::current_dir().unwrap_or_default();
    let entries: Vec<Value> = jobs
        .iter()
        .filter_map(|job| {
            let cc = job.get("cc").and_then(Value::as_str)?;
            let src = job.get("src").and_then(Value::as_str)?;
            let obj = job.get("obj").and_then(Value::as_str)?;
            let flags: Vec<String> = job
                .get("flags")
                .and_then(Value::as_array)
                .map(|flags| {
                    flags
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let mut arguments = vec![cc.to_owned()];
            arguments.extend(flags);
            arguments.extend([
                "-c".to_owned(),
                src.to_owned(),
                "-o".to_owned(),
                obj.to_owned(),
            ]);
            Some(json!({
                "directory": directory.to_string_lossy(),
                "file": src,
                "output": obj,
                "arguments": arguments,
            }))
        })
        .collect();
    let mut merged: Vec<Value> = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let outputs: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(|entry| entry["output"].as_str().map(str::to_owned))
        .collect();
    merged.retain(|entry| {
        entry["output"]
            .as_str()
            .is_some_and(|output| !outputs.contains(output))
            && entry["file"]
                .as_str()
                .is_some_and(|file| Path::new(file).is_file())
    });
    merged.extend(entries);
    merged.sort_by(|a, b| {
        a["file"]
            .as_str()
            .unwrap_or_default()
            .cmp(b["file"].as_str().unwrap_or_default())
            .then_with(|| {
                a["output"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(b["output"].as_str().unwrap_or_default())
            })
    });
    let temporary = path.with_extension("json.new");
    let write = || -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&temporary, serde_json::to_vec(&merged)?)?;
        fs::rename(&temporary, path)
    };
    if let Err(error) = write() {
        let _ = fs::remove_file(&temporary);
        return Err(format!("{}: {error}", path.display()));
    }
    Ok(())
}

/// Compile one source. Returns whether it was built now or came from the
/// stamp/cache, how long it took, and the compiler's error if any.
/// Which jobs this run really compiles: an object stamped with its key, or
/// one already in the shared cache, is only linked in.
pub fn plan_json(request: &Value, root: &Path) -> Result<Value, String> {
    let jobs = request
        .get("jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let cache = PathBuf::from(request.get("cache").and_then(Value::as_str).unwrap_or(""));
    let mut hashes = Hashes::load(&cache);
    let mut work = Vec::new();
    let mut keys = Vec::new();
    for job in jobs {
        let source = PathBuf::from(job.get("src").and_then(Value::as_str).unwrap_or(""));
        let object = PathBuf::from(job.get("obj").and_then(Value::as_str).unwrap_or(""));
        let flags: Vec<String> = job
            .get("flags")
            .and_then(Value::as_array)
            .map(|flags| {
                flags
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let deps = match job.get("deps_file").and_then(Value::as_str) {
            Some(path) if !path.is_empty() => fs::read(path).map_err(|e| format!("{path}: {e}"))?,
            _ => Vec::new(),
        };
        let key = hashes.key_of(&source, &flags, &deps, root)?;
        keys.push(
            json!({"src": source.to_string_lossy(), "obj": object.to_string_lossy(), "key": key}),
        );
        let stamp = append(&object, ".sha1");
        let stamped = object.is_file()
            && fs::read_to_string(&stamp)
                .map(|old| old == key)
                .unwrap_or(false);
        let cached = cache.join(format!("{key}.o")).is_file();
        if !stamped && !cached {
            work.push(job);
        }
    }
    if !cache.as_os_str().is_empty() {
        hashes.save(); // (the sources this plan hashed, for the next build)
    }
    Ok(json!({"work": work, "keys": keys}))
}

/// How long the rest of the compile takes with this many compilers.
pub fn eta_left(pending: &[f64], jobs: usize) -> f64 {
    if pending.is_empty() {
        return 0.0;
    }
    let longest = pending.iter().copied().fold(0.0f64, f64::max);
    longest.max(pending.iter().sum::<f64>() / jobs.max(1) as f64)
}

/// The memory a compile takes at its peak, in MB: fitted to a cold build of
/// the lifted code (clang 23, x86-64). A chunk of ordinary functions is
/// ~1.4 GB for 25 MB; a compile unit of 6 MB ~0.45 GB; a single giant function
/// adds ~250 MB for each of its megabytes (a 6 MB function alone took 2.2 GB).
/// 120 MB is clang's own and the object writer's.
pub fn compile_need_mb(source: &Path) -> u64 {
    let Ok(text) = fs::read_to_string(source) else {
        return 1500;
    };
    let mb = text.len() as f64 / (1u64 << 20) as f64;
    let largest = crate::lifted_c::spans_noted(&text)
        .iter()
        .map(|span| (span.end - span.start) as f64)
        .fold(0.0, f64::max)
        / (1u64 << 20) as f64;
    (120.0 + 55.0 * mb + 250.0 * largest).ceil() as u64
}

/// The memory kept free for the rest of the PC, in MB.
pub const RESERVE_MB: u64 = 1024;

/// How many compiles to run at once: the most whose peaks, largest first, fit
/// in the memory available (less RESERVE_MB), at most one a core. The peaks
/// are a conservative bound: the n jobs a build starts first are its n
/// biggest. None for the memory (not known): every core.
pub fn parallel_for(needs: &[u64], available_mb: Option<u64>, cores: usize) -> usize {
    let cores = cores.max(1);
    let Some(available) = available_mb else {
        return cores;
    };
    let budget = available.saturating_sub(RESERVE_MB);
    let mut peaks = needs.to_vec();
    peaks.sort_unstable_by(|a, b| b.cmp(a));
    let mut sum = 0u64;
    let mut count = 0usize;
    for need in peaks.iter().take(cores) {
        sum += need;
        if sum > budget {
            break;
        }
        count += 1;
    }
    count.max(1).min(cores)
}

/// How many jobs each wave runs: the memory gate's count for the largest
/// remaining job and those after it. A build's first wave is its giant
/// functions (2-3 GB each, so few); once they are done the ordinary 6 MB
/// units (~0.5 GB each) fit on most of the cores instead of the four the
/// giants allowed for the whole build.
pub fn wave_counts(needs: &[u64], available_mb: Option<u64>, cores: usize) -> Vec<usize> {
    let mut counts = Vec::new();
    let mut at = 0;
    while at < needs.len() {
        let n = parallel_for(&needs[at..], available_mb, cores).min(needs.len() - at);
        counts.push(n);
        at += n;
    }
    counts
}

/// Compile a list of jobs, longest first, up to `jobs` at once. With
/// `needs` (the jobs' memory, in the same order) and memory known, the
/// jobs run in waves whose size the memory gate decides as the giants
/// finish; otherwise `parallel` jobs run all the time.
pub fn many_json(request: &Value, root: &Path) -> Result<Value, String> {
    let jobs: Vec<Value> = request
        .get("jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let cache = PathBuf::from(request.get("cache").and_then(Value::as_str).unwrap_or(""));
    let hashes = (!cache.as_os_str().is_empty()).then(|| Mutex::new(Hashes::load(&cache)));
    let asked = request.get("parallel").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = if asked == 0 {
        crate::toolchain::default_jobs(3.5)
    } else {
        asked
    };
    let cores = if asked == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        asked
    };
    let needs_requested: Option<Vec<u64>> =
        request.get("needs").and_then(Value::as_array).map(|needs| {
            needs
                .iter()
                .map(|need| need.as_u64().unwrap_or(0))
                .collect()
        });
    let mut paired: Vec<(Value, u64)> = jobs
        .into_iter()
        .enumerate()
        .map(|(index, job)| {
            let need = needs_requested
                .as_ref()
                .and_then(|needs| needs.get(index))
                .copied()
                .unwrap_or(0);
            (job, need)
        })
        .collect();
    paired.sort_by_key(|(job, _)| {
        std::cmp::Reverse(
            job.get("src")
                .and_then(Value::as_str)
                .and_then(|path| fs::metadata(path).ok())
                .map(|meta| meta.len())
                .unwrap_or(0),
        )
    });
    let needs: Vec<u64> = paired.iter().map(|(_, need)| *need).collect();
    let jobs: Vec<Value> = paired.into_iter().map(|(job, _)| job).collect();
    let memory_scheduled = needs_requested
        .as_ref()
        .is_some_and(|n| n.len() == jobs.len())
        && needs.iter().any(|need| *need > 0);
    let started = Instant::now();
    let mut compiled = 0usize;
    let mut cached = 0usize;
    let mut failed = Vec::new();
    let mut results: Vec<(usize, Result<Value, String>)> = Vec::new();
    let run = |first: usize, workers: usize, end: usize| -> Vec<(usize, Result<Value, String>)> {
        let next = AtomicUsize::new(first);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    let next = &next;
                    let jobs = &jobs;
                    let hashes = hashes.as_ref();
                    scope.spawn(move || {
                        let mut out = Vec::new();
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            if index >= end {
                                break;
                            }
                            out.push((index, compile_one_with(&jobs[index], root, hashes)));
                        }
                        out
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .flatten()
                .collect()
        })
    };
    if memory_scheduled && !jobs.is_empty() {
        let available = request
            .get("available_mb")
            .and_then(Value::as_u64)
            .or_else(crate::toolchain::available_mb);
        let mut at = 0;
        for count in wave_counts(&needs, available, cores) {
            results.extend(run(at, count, at + count));
            at += count;
        }
    } else if !jobs.is_empty() {
        results.extend(run(0, limit.min(jobs.len()), jobs.len()));
    }
    if let Some(hashes) = hashes {
        hashes
            .into_inner()
            .unwrap_or_else(|e| e.into_inner())
            .save();
    }
    let keys: Vec<Value> = results
        .iter()
        .filter_map(|(index, result)| {
            let key = result.as_ref().ok()?.get("key")?.as_str()?;
            Some(json!({
                "src": jobs[*index]["src"],
                "obj": jobs[*index]["obj"],
                "key": key,
            }))
        })
        .collect();
    for (index, result) in results {
        match result {
            Ok(value) if value["compiled"] == Value::Bool(true) => compiled += 1,
            Ok(value) if value["cached"] == Value::Bool(true) => cached += 1,
            Ok(value) => failed.push(json!({
                "index": index,
                "src": jobs[index]["src"],
                "error": value["error"],
            })),
            Err(error) => failed.push(json!({
                "index": index,
                "src": jobs[index]["src"],
                "error": error,
            })),
        }
    }
    Ok(json!({
        "planned": jobs.len(),
        "compiled": compiled,
        "cached": cached,
        "failed": failed,
        "keys": keys,
        "seconds": started.elapsed().as_secs_f64(),
    }))
}

pub fn compile_one_json(request: &Value, root: &Path) -> Result<Value, String> {
    let cache = request
        .get("cache")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if cache.is_empty() {
        return compile_one_with(request, root, None);
    }
    let hashes = Mutex::new(Hashes::load(Path::new(cache)));
    let result = compile_one_with(request, root, Some(&hashes));
    hashes
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .save();
    result
}

fn compile_one_with(
    request: &Value,
    root: &Path,
    hashes: Option<&Mutex<Hashes>>,
) -> Result<Value, String> {
    let text = |key: &str| {
        request
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let source = PathBuf::from(text("src"));
    let object = PathBuf::from(text("obj"));
    let compiler = text("cc");
    let cache = PathBuf::from(text("cache"));
    if source.as_os_str().is_empty() || object.as_os_str().is_empty() || compiler.is_empty() {
        return Err("build.compile_one needs cc, src, obj and cache".into());
    }
    let flags: Vec<String> = request
        .get("flags")
        .and_then(Value::as_array)
        .map(|flags| {
            flags
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let deps = match text("deps_file") {
        path if path.is_empty() => Vec::new(),
        path => fs::read(&path).map_err(|e| format!("{path}: {e}"))?,
    };
    let key = match hashes {
        Some(hashes) => hashes
            .lock()
            .map_err(|_| "the build's hash record is poisoned".to_owned())?
            .key_of(&source, &flags, &deps, root)?,
        None => object_key(&source, &flags, &deps, root)?,
    };
    if let Some(parent) = object.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let stamp = append(&object, ".sha1");
    if object.is_file()
        && fs::read_to_string(&stamp)
            .map(|old| old == key)
            .unwrap_or(false)
    {
        return Ok(
            json!({"compiled": false, "cached": true, "seconds": 0.0, "key": key, "error": Value::Null}),
        );
    }
    let cached = cache.join(format!("{key}.o"));
    if cached.is_file() {
        if object.exists() {
            fs::remove_file(&object).map_err(|e| format!("{}: {e}", object.display()))?;
        }
        if fs::hard_link(&cached, &object).is_err() {
            fs::copy(&cached, &object).map_err(|e| format!("{}: {e}", object.display()))?;
        }
        fs::write(&stamp, &key).map_err(|e| format!("{}: {e}", stamp.display()))?;
        return Ok(
            json!({"compiled": false, "cached": true, "seconds": 0.0, "key": key, "error": Value::Null}),
        );
    }
    let temporary = append(&object, ".part");
    let started = Instant::now();
    let output = Command::new(&compiler)
        .args(&flags)
        .arg("-c")
        .arg(&source)
        .arg("-o")
        .arg(&temporary)
        .output()
        .map_err(|e| format!("{compiler}: {e}"))?;
    let seconds = started.elapsed().as_secs_f64();
    if !output.status.success() {
        let _ = fs::remove_file(&temporary);
        let mut error = String::from_utf8_lossy(&output.stderr).into_owned();
        error.push_str(&String::from_utf8_lossy(&output.stdout));
        if error.trim().is_empty() {
            error = format!(
                "Compiler exited with status {:?} without diagnostics.",
                output.status.code()
            );
        }
        let start = error.len().saturating_sub(4000);
        return Ok(
            json!({"compiled": false, "cached": false, "seconds": seconds, "key": key, "error": &error[start..]}),
        );
    }
    fs::rename(&temporary, &object).map_err(|e| format!("{}: {e}", object.display()))?;
    if deps.starts_with(DEPFILE_DEPS) {
        // (the planned key came from the last compile's depfile, which may lack
        // a header this source reads now: the key does not vouch for the
        // object. rekey_object names it by the depfile this compile wrote; a
        // build that stops before that leaves it pending, and the next build
        // compiles it again)
        fs::write(&stamp, DEPFILE_PENDING).map_err(|e| format!("{}: {e}", stamp.display()))?;
        return Ok(
            json!({"compiled": true, "cached": false, "seconds": seconds, "key": key, "error": Value::Null}),
        );
    }
    fs::write(&stamp, &key).map_err(|e| format!("{}: {e}", stamp.display()))?;
    cache_put(&object, &key, &cache)?;
    Ok(
        json!({"compiled": true, "cached": false, "seconds": seconds, "key": key, "error": Value::Null}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_object_keys_and_a_known_source_is_not_read_again() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-hash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("a.c");
        fs::write(&source, "int a;\n").unwrap();
        let cache = base.join("cache");
        let flags = ["-O1".to_owned()];
        let deps = b"the headers";
        let mut hashes = Hashes::load(&cache);
        let key = hashes.key_of(&source, &flags, deps, &base).unwrap();
        assert_eq!(key, object_key(&source, &flags, deps, &base).unwrap());
        hashes.save();
        assert!(cache.join(HASH_CACHE).is_file());
        // (a later build: the record is older than the racy window)
        let mut again = Hashes::load(&cache);
        again.written_ns = now_ns() + 5 * RACY_NS;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
            assert_eq!(
                again.key_of(&source, &flags, deps, &base).unwrap(),
                key,
                "a known source is not read"
            );
            fs::set_permissions(&source, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn another_size_or_time_and_other_flags_read_again() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-hash2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("b.c");
        fs::write(&source, "int b;\n").unwrap();
        let cache = base.join("cache");
        let mut hashes = Hashes::load(&cache);
        let one = ["-O1".to_owned()];
        let two = ["-O2".to_owned()];
        let first = hashes.key_of(&source, &one, b"", &base).unwrap();
        let second = hashes.key_of(&source, &two, b"", &base).unwrap();
        assert_ne!(first, second, "other flags are another key");
        assert_eq!(hashes.key_of(&source, &one, b"", &base).unwrap(), first);
        assert!(
            hashes.files[&source.to_string_lossy().into_owned()]
                .keys
                .values()
                .any(|key| *key == first)
        );
        assert!(
            hashes.files[&source.to_string_lossy().into_owned()]
                .keys
                .values()
                .any(|key| *key == second)
        );
        let longer = "int b; /* a longer source */\n";
        fs::write(&source, longer).unwrap();
        let changed = hashes.key_of(&source, &one, b"", &base).unwrap();
        assert_ne!(changed, first, "another size reads the source again");
        let entry = &hashes.files[&source.to_string_lossy().into_owned()];
        assert_eq!(entry.stat.size, longer.len() as u64);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_racy_record_is_not_trusted_and_other_content_loses_its_keys() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-hash3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("c.c");
        fs::write(&source, "int c;\n").unwrap();
        let mut hashes = Hashes::load(&base.join("cache"));
        let flags = ["-O1".to_owned()];
        let key = hashes.key_of(&source, &flags, b"", &base).unwrap();
        // (the file was written beside the record: a change within the clock's
        // step keeps its size and time, so it is read again)
        hashes.written_ns = now_ns();
        let entry = hashes
            .files
            .get_mut(&source.to_string_lossy().into_owned())
            .unwrap();
        entry.sha1 = "a record of other content".to_owned();
        entry.keys.insert("a part".to_owned(), key.clone());
        let again = hashes.key_of(&source, &flags, b"", &base).unwrap();
        assert_eq!(again, key);
        let entry = &hashes.files[&source.to_string_lossy().into_owned()];
        assert_ne!(entry.sha1, "a record of other content");
        assert!(!entry.keys.contains_key("a part"), "untrusted keys go");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_broken_record_or_another_scheme_is_ignored_and_a_gone_file_leaves() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-hash4-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("cache")).unwrap();
        let path = base.join("cache").join(HASH_CACHE);
        fs::write(&path, "not json").unwrap();
        assert!(Hashes::load(&base.join("cache")).files.is_empty());
        fs::write(&path, r#"{"scheme":99,"written_ns":1,"files":{}}"#).unwrap();
        assert!(Hashes::load(&base.join("cache")).files.is_empty());
        let mut hashes = Hashes::load(&base.join("cache"));
        hashes.files.insert(
            base.join("gone.c").to_string_lossy().into_owned(),
            HashEntry {
                stat: FileStat {
                    size: 1,
                    mtime_ns: 1,
                    id: 1,
                },
                sha1: "gone".to_owned(),
                keys: BTreeMap::new(),
            },
        );
        hashes.save();
        assert!(Hashes::load(&base.join("cache")).files.is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_plan_records_its_keys_for_the_next_build() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-plan-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("p.c");
        fs::write(&source, "int p;\n").unwrap();
        let cache = base.join("cache");
        let job = json!({"cc": "cc", "src": source, "obj": base.join("p.o"), "cache": cache, "flags": ["-O1"], "deps_file": ""});
        let request = json!({"jobs": [job], "cache": cache});
        let plan = plan_json(&request, &base).unwrap();
        let key = plan["keys"][0]["key"].as_str().unwrap();
        assert!(cache.join(HASH_CACHE).is_file());
        let mut hashes = Hashes::load(&cache);
        hashes.written_ns = now_ns() + 5 * RACY_NS;
        assert_eq!(
            hashes
                .key_of(&source, &["-O1".to_owned()], b"", &base)
                .unwrap(),
            key
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn waves_use_the_cores_the_giants_were_holding_back() {
        // (two 3 GB functions fit in 8 GB less the reserve, with room for
        // one 0.5 GB unit; the seven small units left then all fit)
        let needs = vec![3000, 3000, 500, 500, 500, 500, 500, 500, 500];
        assert_eq!(wave_counts(&needs, Some(8_000), 8), vec![3, 6]);
        // (four giants: two a wave, then one giant with both small units,
        // then the last unit)
        let needs = vec![3000, 3000, 3000, 3000, 500, 500];
        assert_eq!(wave_counts(&needs, Some(8_000), 8), vec![2, 3, 1]);
        // (unknown memory: one wave of every core; never zero jobs)
        assert_eq!(wave_counts(&[100, 100, 100], None, 4), vec![3]);
        assert_eq!(wave_counts(&[], Some(8_000), 8), Vec::<usize>::new());
    }

    #[test]
    fn eta_is_the_slower_of_the_work_and_the_longest_file() {
        assert_eq!(eta_left(&[], 4), 0.0);
        assert_eq!(eta_left(&[10.0, 2.0, 2.0], 2), 10.0);
        assert_eq!(eta_left(&[4.0, 4.0, 4.0], 2), 6.0);
    }

    #[test]
    fn every_job_runs_even_with_fewer_workers() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-many-fixed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let cache = base.join("cache");
        let mut jobs = Vec::new();
        for i in 0..5 {
            let source = base.join(format!("f{i}.c"));
            fs::write(
                &source,
                format!("int hookshot_fixed_{i}(void) {{ return {i}; }}\n"),
            )
            .unwrap();
            jobs.push(json!({"cc": "cc", "src": source, "obj": base.join(format!("f{i}.o")), "cache": cache, "flags": ["-O0"], "deps_file": ""}));
        }
        let request = json!({"jobs": jobs, "parallel": 2, "cache": cache});
        let answer = many_json(&request, &base).unwrap();
        if answer["failed"]
            .as_array()
            .is_some_and(|failed| !failed.is_empty())
        {
            let _ = fs::remove_dir_all(base);
            return; // no C compiler on this machine
        }
        assert_eq!(answer["planned"], 5);
        assert_eq!(answer["compiled"], 5);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn the_job_count_is_what_the_memory_holds() {
        // (unknown memory: every core)
        assert_eq!(parallel_for(&[500, 500], None, 8), 8);
        // (6 MB compile units: 0.45 GB each, 13 GB free: every core)
        let units = vec![460u64; 300];
        assert_eq!(parallel_for(&units, Some(14_000), 16), 16);
        // (25 MB chunks: 1.45 GB each, 12 GB free: seven)
        let chunks = vec![1450u64; 59];
        assert_eq!(parallel_for(&chunks, Some(12_000), 16), 7);
        // (a giant function's unit: two of 3 GB fit in 8 GB, less the reserve)
        assert_eq!(parallel_for(&[3000, 3000], Some(8_000), 16), 2);
        // (never zero, never more than the cores)
        assert_eq!(parallel_for(&[9000], Some(4_000), 16), 1);
        assert_eq!(parallel_for(&[], Some(14_000), 16), 1);
        assert_eq!(parallel_for(&[100; 10], Some(64_000), 4), 4);
    }

    #[test]
    fn a_compile_need_grows_with_the_file_and_its_largest_function() {
        let base = std::env::temp_dir().join(format!("hookshot-need-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let small = base.join("small.c");
        let mut text = String::from("#include \"a64.h\"\n");
        for i in 0..100 {
            text.push_str(&format!(
                "void f_{:08x}(Ctx *__restrict A64_NOESC c) {{\n{}\n}}\n",
                0x80000000u32 + i * 16,
                "x".repeat(10_000)
            ));
        }
        fs::write(&small, &text).unwrap();
        let unit = compile_need_mb(&small);
        assert!(unit > 120 && unit < 700, "{unit} MB");
        let giant = base.join("giant.c");
        let mut one = String::from("#include \"a64.h\"\n");
        one.push_str("void f_80000000(Ctx *__restrict A64_NOESC c) {\n");
        one.push_str(&"x".repeat(4 << 20));
        one.push_str("\n}\n");
        fs::write(&giant, &one).unwrap();
        let huge = compile_need_mb(&giant);
        assert!(huge > 1000, "{huge} MB for a 4 MB function");
        assert!(compile_need_mb(&base.join("missing.c")) >= 400);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn plan_skips_a_stamped_or_cached_object() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("a.c");
        fs::write(&source, "int a;\n").unwrap();
        let object = base.join("a.o");
        let cache = base.join("cache");
        let job = json!({"cc": "cc", "src": source, "obj": object, "cache": cache, "flags": ["-O1"], "deps_file": ""});
        let request = json!({"jobs": [job.clone()], "cache": cache});
        let plan = plan_json(&request, &base).unwrap();
        assert_eq!(plan["work"].as_array().unwrap().len(), 1);
        let key = plan["keys"][0]["key"].as_str().unwrap();
        fs::write(&object, b"object").unwrap();
        let mut stamp = object.as_os_str().to_owned();
        stamp.push(".sha1");
        fs::write(stamp, key).unwrap();
        let plan = plan_json(&request, &base).unwrap();
        assert_eq!(plan["work"].as_array().unwrap().len(), 0);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn flags_ignore_includes_and_paths() {
        let flags = vec![
            "-O1".to_owned(),
            "-I/headers".to_owned(),
            "--sysroot=/tmp/mount".to_owned(),
            "/repo/host".to_owned(),
        ];
        assert_eq!(
            flag_text(&flags, Path::new("/repo")),
            "-O1 --sysroot=<sysroot> <root>/host"
        );
    }

    #[test]
    fn compiles_several_jobs_and_reuses_them() {
        let base = std::env::temp_dir().join(format!("hookshot-rust-many-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let cache = base.join("cache");
        let mut jobs = Vec::new();
        for i in 0..2 {
            let source = base.join(format!("m{i}.c"));
            fs::write(
                &source,
                format!("int hookshot_many_{i}(void) {{ return {i}; }}\n"),
            )
            .unwrap();
            jobs.push(json!({"cc": "cc", "src": source, "obj": base.join(format!("m{i}.o")), "cache": cache, "flags": ["-O0"], "deps_file": ""}));
        }
        let request = json!({"jobs": jobs, "parallel": 2, "cache": cache});
        let first = many_json(&request, &base).unwrap();
        if first["failed"]
            .as_array()
            .map(|failed| !failed.is_empty())
            .unwrap_or(false)
        {
            let _ = fs::remove_dir_all(base);
            return; // no C compiler on this machine
        }
        assert_eq!(first["planned"], 2);
        assert_eq!(first["compiled"], 2);
        let again = many_json(&request, &base).unwrap();
        assert_eq!(again["cached"], 2);
        assert_eq!(again["compiled"], 0);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn compiles_once_then_uses_the_stamp() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-compile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("a.c");
        fs::write(&source, "int hookshot_test(void) { return 1; }\n").unwrap();
        let object = base.join("a.o");
        let cache = base.join("cache");
        let request = |key: &str| json!({"cc": "cc", "src": source, "obj": object, "cache": cache, "flags": [key], "deps_file": ""});
        let first = compile_one_json(&request("-O0"), &base).unwrap();
        if first["error"] != Value::Null {
            return; // no C compiler on this machine
        }
        assert_eq!(first["compiled"], true);
        assert!(object.is_file());
        let key = first["key"].as_str().unwrap().to_owned();
        assert!(cache.join(format!("{key}.o")).is_file());
        let again = compile_one_json(&request("-O0"), &base).unwrap();
        assert_eq!(again["compiled"], false);
        assert_eq!(again["cached"], true);
        let changed = compile_one_json(&request("-O2"), &base).unwrap();
        if changed["error"] == Value::Null {
            assert_eq!(changed["compiled"], true);
            assert_ne!(changed["key"], first["key"]);
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn the_depfile_flag_is_not_part_of_the_object_key() {
        let plain = vec!["-O1".to_owned(), "-I/headers".to_owned()];
        let with_md = vec!["-O1".to_owned(), "-I/headers".to_owned(), "-MD".to_owned()];
        assert_eq!(
            flag_text(&plain, Path::new("/repo")),
            flag_text(&with_md, Path::new("/repo"))
        );
        assert!(!flag_text(&with_md, Path::new("/repo")).contains("-MD"));
    }

    #[test]
    fn a_depfile_keyed_object_waits_for_its_own_depfile_and_is_rekeyed() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-depfile-key-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("a.c");
        fs::write(&source, "int a;\n").unwrap();
        let object = base.join("a.o");
        fs::write(&object, b"object").unwrap();
        let stamp = append(&object, ".sha1");
        let flags = vec!["-O1".to_owned(), "-MD".to_owned()];
        let cache = base.join("cache");
        fs::write(&stamp, DEPFILE_PENDING).unwrap();
        let new_deps = b"depfile:the headers the new compile read";
        let key = rekey_object(&source, &object, &flags, new_deps, &base, &cache, "old")
            .unwrap()
            .expect("a pending object is this build's");
        assert_eq!(key, object_key(&source, &flags, new_deps, &base).unwrap());
        assert_eq!(fs::read_to_string(&stamp).unwrap(), key);
        assert!(cache.join(format!("{key}.o")).is_file());
        // (an object stamped by another build is left alone)
        fs::write(&stamp, "another build's key").unwrap();
        assert_eq!(
            rekey_object(&source, &object, &flags, new_deps, &base, &cache, "old").unwrap(),
            None
        );
        assert_eq!(fs::read_to_string(&stamp).unwrap(), "another build's key");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_compile_from_a_depfile_key_is_pending_and_not_cached() {
        let base = std::env::temp_dir().join(format!(
            "hookshot-rust-depfile-pending-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let source = base.join("a.c");
        fs::write(&source, "int hookshot_pending(void) { return 1; }\n").unwrap();
        let deps = base.join("deps.bin");
        fs::write(&deps, b"depfile:the last compile's headers").unwrap();
        let object = base.join("a.o");
        let cache = base.join("cache");
        let request = json!({
            "cc": "cc", "src": source, "obj": object, "cache": cache,
            "flags": ["-O0", "-MD"], "deps_file": deps,
        });
        let answer = compile_one_json(&request, &base).unwrap();
        if answer["error"] != Value::Null {
            let _ = fs::remove_dir_all(base);
            return; // no C compiler on this machine
        }
        assert_eq!(answer["compiled"], true);
        let stamp = append(&object, ".sha1");
        assert_eq!(fs::read_to_string(&stamp).unwrap(), DEPFILE_PENDING);
        assert!(object.is_file());
        let key = answer["key"].as_str().unwrap();
        assert!(
            !cache.join(format!("{key}.o")).is_file(),
            "a key from the last depfile does not vouch for this object"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn compile_commands_have_the_clangd_schema_and_merge_stages() {
        let base =
            std::env::temp_dir().join(format!("hookshot-rust-compdb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let one = base.join("one.c");
        let two = base.join("two.cpp");
        fs::write(&one, "int one;\n").unwrap();
        fs::write(&two, "int two;\n").unwrap();
        let path = base.join(COMPDB);
        let jobs = vec![json!({
            "cc": "clang", "src": one, "obj": base.join("one.o"),
            "flags": ["-O1", "-Ifoo"],
        })];
        write_compdb(&path, &jobs).unwrap();
        let entries: Vec<Value> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["file"], one.to_string_lossy().as_ref());
        assert_eq!(
            entries[0]["output"],
            base.join("one.o").to_string_lossy().as_ref()
        );
        assert_eq!(
            entries[0]["arguments"],
            json!([
                "clang",
                "-O1",
                "-Ifoo",
                "-c",
                one.to_string_lossy(),
                "-o",
                base.join("one.o").to_string_lossy()
            ])
        );
        assert_eq!(
            entries[0]["directory"],
            std::env::current_dir().unwrap().to_string_lossy().as_ref()
        );
        // (a later stage adds its jobs: the earlier stage's entry stays)
        let jobs = vec![json!({
            "cc": "clang++", "src": two, "obj": base.join("two.o"),
            "flags": ["-O2"],
        })];
        write_compdb(&path, &jobs).unwrap();
        let entries: Vec<Value> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .any(|entry| entry["file"] == one.to_string_lossy().as_ref())
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry["file"] == two.to_string_lossy().as_ref())
        );
        let _ = fs::remove_dir_all(base);
    }
}
