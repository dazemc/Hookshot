//! Link response files and the load-time symbol check for hookshot_vk.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

pub fn write_rsp(path: &Path, objects: &[String]) -> Result<(), String> {
    let text = objects
        .iter()
        .map(|object| format!("\"{}\"", object.replace('\\', "/")))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn nm() -> Option<PathBuf> {
    for name in ["nm", "llvm-nm"] {
        if let Some(path) = crate::toolchain::find(name, true)
            .to_str()
            .filter(|path| Path::new(path).is_file())
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// Undefined, non-weak symbols that are not the C/C++ runtime's: what
/// dlopen(RTLD_NOW) would refuse.
pub fn undefined_symbols(shared_object: &Path) -> Result<Vec<String>, String> {
    let Some(nm) = nm() else {
        return Ok(Vec::new());
    };
    let output = Command::new(&nm)
        .args(["-D", "--undefined-only"])
        .arg(shared_object)
        .output()
        .map_err(|e| format!("{}: {e}", nm.display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut bad = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        let (Some(kind), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if kind != "U" {
            continue;
        }
        if ["@GLIBC", "@CXXABI", "@GLIBCXX", "@GCC"]
            .iter()
            .any(|marker| name.contains(marker))
        {
            continue;
        }
        bad.push(name.to_owned());
    }
    Ok(bad)
}

pub fn check_symbols_json(shared_object: &str) -> Result<Value, String> {
    let path = Path::new(shared_object);
    let bad = undefined_symbols(path)?;
    Ok(json!({
        "ok": bad.is_empty(),
        "file": shared_object,
        "undefined": bad,
        "message": if bad.is_empty() {
            format!("{} loads with every symbol resolved", path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default())
        } else {
            format!("{} has {} unresolved symbol(s); it would not load", path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default(), bad.len())
        },
    }))
}

fn link_argv(args: &[String], response: &Path, after: &[String]) -> Vec<String> {
    let mut argv = args.to_vec();
    argv.push(format!("@{}", response.display()));
    argv.extend(after.iter().cloned());
    argv
}

/// Run a linker command with an object response file.
pub fn link_json(request: &Value) -> Result<Value, String> {
    let cc = request.get("cc").and_then(Value::as_str).unwrap_or("cc");
    let out = request
        .get("out")
        .and_then(Value::as_str)
        .unwrap_or("a.out");
    let objects: Vec<String> = request
        .get("objects")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let args: Vec<String> = request
        .get("args")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    // Libraries after the objects: a static library named before them is not
    // searched for what they need. nx2/build.py writes "@objs.rsp" between the
    // output options and HOST_LIBS in the same way.
    let after: Vec<String> = request
        .get("after")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let rsp = PathBuf::from(format!("{out}.rsp"));
    write_rsp(&rsp, &objects)?;
    let output = Command::new(cc)
        .args(link_argv(&args, &rsp, &after))
        .output()
        .map_err(|e| format!("{cc}: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let undefined = text
        .lines()
        .filter(|line| line.contains("undefined symbol"))
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": output.status.success(),
        "out": out,
        "undefined": undefined,
        "output": text.chars().rev().take(3000).collect::<String>().chars().rev().collect::<String>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_forward_slash_paths() {
        let path =
            std::env::temp_dir().join(format!("hookshot-rust-rsp-{}.rsp", std::process::id()));
        write_rsp(&path, &["C:\\a b\\x.o".into(), "/tmp/y.o".into()]).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "\"C:/a b/x.o\"\n\"/tmp/y.o\""
        );
        write_rsp(&path, &["/tmp/é 游戏/y.o".into()]).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "\"/tmp/é 游戏/y.o\"");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn objects_come_between_the_options_and_the_libraries() {
        let argv = link_argv(
            &["-o".to_owned(), "hookshot.exe".to_owned()],
            Path::new("objs.rsp"),
            &["-lwinhttp".to_owned(), "-lc++".to_owned()],
        );
        assert_eq!(
            argv,
            vec!["-o", "hookshot.exe", "@objs.rsp", "-lwinhttp", "-lc++"]
        );
    }

    #[test]
    fn finds_an_undefined_symbol() {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-undef-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let source = folder.join("lib.c");
        fs::write(&source, "extern void hookshot_missing(void); void hookshot_call(void) { hookshot_missing(); }\n").unwrap();
        let object = folder.join("lib.so");
        let status = Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&object)
            .arg(&source)
            .status();
        if !status.map(|status| status.success()).unwrap_or(false) {
            return;
        }
        let bad = undefined_symbols(&object).unwrap();
        assert!(bad.iter().any(|name| name == "hookshot_missing"));
        let _ = fs::remove_dir_all(folder);
    }
}
