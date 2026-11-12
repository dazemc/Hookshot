//! One compile with the object cache nx2/build.py keeps: an object is
//! rebuilt only when its source, flags or headers changed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde_json::{Value, json};
use sha1::{Digest, Sha1};

/// The flags an object key is named by: include folders are left out (their
/// headers are in deps), the builder's folder is <root>, and a sysroot's
/// mount path is <sysroot>.
pub fn flag_text(flags: &[String], root: &Path) -> String {
    let root = root.to_string_lossy();
    flags
        .iter()
        .filter(|flag| !flag.starts_with("-I"))
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
    let mut digest = Sha1::new();
    digest.update(data);
    digest.update(flag_text(flags, root).as_bytes());
    digest.update(deps);
    Ok(format!("{:x}", digest.finalize()))
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
        let key = object_key(&source, &flags, &deps, root)?;
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

/// Compile a list of jobs, longest first, up to `jobs` at once./// Compile a list of jobs, longest first, up to `jobs` at once.
pub fn many_json(request: &Value, root: &Path) -> Result<Value, String> {
    let jobs: Vec<Value> = request
        .get("jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let limit = request.get("parallel").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = if limit == 0 {
        crate::toolchain::default_jobs(3.5)
    } else {
        limit
    };
    let mut jobs = jobs;
    jobs.sort_by_key(|job| {
        std::cmp::Reverse(
            job.get("src")
                .and_then(Value::as_str)
                .and_then(|path| fs::metadata(path).ok())
                .map(|meta| meta.len())
                .unwrap_or(0),
        )
    });
    let started = Instant::now();
    let mut compiled = 0usize;
    let mut cached = 0usize;
    let mut failed = Vec::new();
    let next = AtomicUsize::new(0);
    let results: Vec<Vec<(usize, Result<Value, String>)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..limit.min(jobs.len()))
            .map(|_| {
                let next = &next;
                let jobs = &jobs;
                scope.spawn(move || {
                    let mut out = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else {
                            break;
                        };
                        out.push((index, compile_one_json(job, root)));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    });
    for (index, result) in results.into_iter().flatten() {
        match result {
            Ok(value) if value["compiled"] == Value::Bool(true) => compiled += 1,
            Ok(value) if value["cached"] == Value::Bool(true) => cached += 1,
            Ok(value) => failed.push(json!({"index": index, "error": value["error"]})),
            Err(error) => failed.push(json!({"index": index, "error": error})),
        }
    }
    Ok(json!({
        "planned": jobs.len(),
        "compiled": compiled,
        "cached": cached,
        "failed": failed,
        "seconds": started.elapsed().as_secs_f64(),
    }))
}

pub fn compile_one_json(request: &Value, root: &Path) -> Result<Value, String> {
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
    let key = object_key(&source, &flags, &deps, root)?;
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
    if let Some(parent) = object.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
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
    fn eta_is_the_slower_of_the_work_and_the_longest_file() {
        assert_eq!(eta_left(&[], 4), 0.0);
        assert_eq!(eta_left(&[10.0, 2.0, 2.0], 2), 10.0);
        assert_eq!(eta_left(&[4.0, 4.0, 4.0], 2), 6.0);
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
}
