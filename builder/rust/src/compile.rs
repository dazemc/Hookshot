//! One compile with the object cache nx2/build.py keeps: an object is
//! rebuilt only when its source, flags or headers changed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
