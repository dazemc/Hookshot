//! Link response files and the load-time symbol check for hookshot_vk.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};
use sha1::{Digest, Sha1};

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
/// A PE DLL's undefined symbols from `llvm-nm -u`: llvm-mingw's lld resolves
/// every import, so a non-empty list means the DLL would not load.
pub fn undefined_symbols_pe(
    shared_object: &Path,
    nm: Option<&Path>,
) -> Result<Vec<String>, String> {
    let nm = match nm.filter(|path| path.is_file()) {
        Some(nm) => nm.to_path_buf(),
        None => match nm_from_mingw(shared_object) {
            Some(nm) => nm,
            None => return Ok(Vec::new()),
        },
    };
    let output = Command::new(&nm)
        .args(["-u"])
        .arg(shared_object)
        .output()
        .map_err(|e| format!("{}: {e}", nm.display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('U')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .collect())
}

/// llvm-nm beside a DLL built by a toolchain a caller did not name: the DLL's
/// folder is not it, so fall back to the PATH's llvm-nm.
fn nm_from_mingw(_shared_object: &Path) -> Option<PathBuf> {
    for name in ["llvm-nm", "x86_64-w64-mingw32-nm"] {
        if let Some(path) = crate::toolchain::find(name, true)
            .to_str()
            .filter(|path| Path::new(path).is_file())
        {
            return Some(PathBuf::from(path));
        }
    }
    None
}

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

fn append(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn which(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.components().count() > 1 {
        return path.is_file().then(|| path.to_path_buf());
    }
    let mut names = vec![name.to_owned()];
    if !name.ends_with(".exe") {
        names.push(format!("{name}.exe"));
    }
    for folder in std::env::split_paths(&std::env::var_os("PATH")?) {
        for name in &names {
            let candidate = folder.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// A file's path, size and modification time, as a link input when it has no
/// object key stamp.
fn stat_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_nanos())
        .unwrap_or(0);
    Some(format!("{} {} {modified}\n", path.display(), meta.len()))
}

/// The files a link reads: the objects of its @response files, and the other
/// existing files it names (not its outputs or maps). A file inside an option
/// is an input too (`-Xlink=/order:@file`, `--symbol-ordering-file=file`).
fn link_inputs(cmd: &[String], output: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let output = std::path::absolute(output).unwrap_or_else(|_| output.to_path_buf());
    let mut objects = Vec::new();
    let mut others = Vec::new();
    let mut seen = HashSet::new();
    for arg in cmd {
        if let Some(response) = arg.strip_prefix('@') {
            if let Ok(text) = fs::read_to_string(response) {
                for line in text.lines() {
                    let object = line.trim().trim_matches('"');
                    if !object.is_empty() {
                        objects.push(PathBuf::from(object));
                    }
                }
            }
            let response = PathBuf::from(response);
            if response.is_file() && seen.insert(response.clone()) {
                others.push(response);
            }
            continue;
        }
        let mut candidates = vec![arg.as_str()];
        if let Some(rest) = arg.strip_prefix("-Xlink=") {
            candidates.push(rest);
        }
        if let Some(rest) = arg.strip_prefix("-Wl,") {
            candidates.extend(rest.split(','));
        }
        if let Some((_, rest)) = arg.split_once('=') {
            candidates.push(rest);
        }
        if let Some((_, rest)) = arg.rsplit_once('@') {
            candidates.push(rest);
        }
        for candidate in candidates {
            let candidate = candidate.strip_prefix('@').unwrap_or(candidate);
            let path = Path::new(candidate);
            if !path.is_file() {
                continue;
            }
            let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            if absolute == output || candidate.ends_with(".map") {
                continue;
            }
            if seen.insert(path.to_path_buf()) {
                others.push(path.to_path_buf());
            }
        }
    }
    (objects, others)
}

/// What a link was made from: its command, the compiler, each object's key
/// (its `.o.sha1` stamp; its size and time without one), and the contents of
/// the other files the command names, inside options too. A link whose inputs
/// are the last successful one's is not run again.
pub fn link_key(cmd: &[String], output: &Path) -> Result<String, String> {
    let mut digest = Sha1::new();
    digest.update(b"hookshot-link-v1\0");
    // (a builder whose link code changed must not keep an old output)
    digest.update(include_bytes!("linker.rs"));
    for arg in cmd {
        digest.update(arg.as_bytes());
        digest.update([0]);
    }
    let cc = which(&cmd[0]).unwrap_or_else(|| PathBuf::from(&cmd[0]));
    if let Some(stat) = stat_text(&cc) {
        digest.update(stat.as_bytes());
    }
    let (objects, others) = link_inputs(cmd, output);
    for object in objects {
        let stamp = append(&object, ".sha1");
        if let Ok(key) = fs::read_to_string(&stamp) {
            digest.update(format!("{} {key}\n", object.display()).as_bytes());
        } else if let Some(stat) = stat_text(&object) {
            digest.update(stat.as_bytes());
        }
    }
    for path in others {
        let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        digest.update(path.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(Sha1::digest(&data));
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// A link's answer: whether it succeeded, its output, and whether it was
/// skipped because its inputs are the last successful link's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkResult {
    pub ok: bool,
    pub output: String,
    pub cached: bool,
}

/// Run a link, or nothing when its inputs are the last successful one's.
/// The key and the output's size are kept beside the output as
/// `<output>.linkkey`; a failed link, or an output of another size, forgets
/// it. `run` is the linker itself, so tests need no compiler.
pub fn run_link_with<F>(cmd: &[String], output: &Path, run: F) -> Result<LinkResult, String>
where
    F: FnOnce(&[String]) -> Result<(bool, String, String), String>,
{
    let keyfile = append(output, ".linkkey");
    let key = link_key(cmd, output).ok();
    if let Some(key) = &key
        && output.is_file()
    {
        let size = fs::metadata(output).map(|meta| meta.len()).unwrap_or(0);
        if let Ok(old) = fs::read_to_string(&keyfile) {
            let mut parts = old.split_whitespace();
            let same_key = parts.next() == Some(key.as_str());
            let same_size = parts
                .next()
                .and_then(|old_size| old_size.parse::<u64>().ok())
                == Some(size);
            if same_key && same_size {
                return Ok(LinkResult {
                    ok: true,
                    output: String::new(),
                    cached: true,
                });
            }
        }
    }
    let _ = fs::remove_file(&keyfile);
    let (ok, stdout, stderr) = run(cmd)?;
    if ok
        && let Some(key) = &key
        && output.is_file()
    {
        let size = fs::metadata(output).map(|meta| meta.len()).unwrap_or(0);
        let _ = fs::write(&keyfile, format!("{key} {size}\n"));
    }
    Ok(LinkResult {
        ok,
        output: format!("{stdout}{stderr}"),
        cached: false,
    })
}

/// `run_link_with` with the real linker.
pub fn run_link(cmd: &[String], output: &Path) -> Result<LinkResult, String> {
    run_link_with(cmd, output, |cmd| {
        let result = Command::new(&cmd[0])
            .args(&cmd[1..])
            .output()
            .map_err(|e| format!("{}: {e}", cmd[0]))?;
        Ok((
            result.status.success(),
            String::from_utf8_lossy(&result.stdout).into_owned(),
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ))
    })
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
    // searched for what they need. build.py writes "@objs.rsp" between the
    // output options and HOST_LIBS in the same way. (libs: the Vulkan build's
    // key; after: the Windows host link's.)
    let mut tail: Vec<String> = request
        .get("libs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    tail.extend(
        request
            .get("after")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    let rsp = PathBuf::from(format!("{out}.rsp"));
    write_rsp(&rsp, &objects)?;
    let mut cmd = vec![cc.to_owned()];
    cmd.extend(link_argv(&args, &rsp, &tail));
    let result = run_link(&cmd, Path::new(out))?;
    let undefined = result
        .output
        .lines()
        .filter(|line| line.contains("undefined symbol"))
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": result.ok,
        "out": out,
        "undefined": undefined,
        "output": result.output.chars().rev().take(3000).collect::<String>().chars().rev().collect::<String>(),
        "cached": result.cached,
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

    fn link_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf, Vec<String>) {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-link-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let object = folder.join("a.o");
        fs::write(&object, b"object").unwrap();
        fs::write(append(&object, ".sha1"), "1".repeat(40)).unwrap();
        let response = folder.join("objs.rsp");
        write_rsp(&response, &[object.to_string_lossy().into_owned()]).unwrap();
        let library = folder.join("lib.a");
        fs::write(&library, b"library").unwrap();
        let order = folder.join("order.txt");
        fs::write(&order, "f_80000000\n").unwrap();
        let output = folder.join("hookshot.so");
        let cmd = vec![
            "cc".to_owned(),
            format!("@{}", response.display()),
            library.to_string_lossy().into_owned(),
            format!("-Xlink=/order:@{}", order.display()),
            "-o".to_owned(),
            output.to_string_lossy().into_owned(),
        ];
        (folder, output, object, library, order, cmd)
    }

    #[test]
    fn the_objects_and_files_inside_options_are_link_inputs() {
        let (folder, output, object, library, order, cmd) = link_fixture("inputs");
        let (objects, others) = link_inputs(&cmd, &output);
        assert_eq!(objects, vec![object.clone()]);
        assert!(others.contains(&library));
        assert!(others.contains(&order));
        assert!(!others.contains(&output));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn unchanged_link_inputs_are_not_linked_again() {
        let (folder, output, object, library, order, cmd) = link_fixture("skip");
        let runs = std::cell::Cell::new(0);
        let link = |fail: bool| {
            let runs = &runs;
            let output = &output;
            run_link_with(&cmd, output, move |_| {
                runs.set(runs.get() + 1);
                fs::write(output, b"linked").unwrap();
                Ok((!fail, String::new(), String::new()))
            })
            .unwrap()
        };
        assert_eq!((link(false).cached, runs.get()), (false, 1));
        assert_eq!((link(false).cached, runs.get()), (true, 1));
        fs::write(append(&object, ".sha1"), "2".repeat(40)).unwrap();
        assert_eq!((link(false).cached, runs.get()), (false, 2));
        fs::write(&library, b"library 2").unwrap();
        assert_eq!((link(false).cached, runs.get()), (false, 3));
        fs::write(&order, "f_80000004\n").unwrap();
        assert_eq!((link(false).cached, runs.get()), (false, 4));
        fs::write(&output, b"another size").unwrap();
        assert_eq!((link(false).cached, runs.get()), (false, 5));
        fs::remove_file(&output).unwrap();
        assert_eq!((link(false).cached, runs.get()), (false, 6));
        // (a failed link forgets the key: the next one runs)
        fs::write(&object, b"object 2").unwrap();
        fs::write(append(&object, ".sha1"), "3".repeat(40)).unwrap();
        assert_eq!((link(true).cached, runs.get()), (false, 7));
        assert!(!append(&output, ".linkkey").exists());
        assert_eq!((link(false).cached, runs.get()), (false, 8));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn the_link_key_follows_each_input() {
        let (folder, output, _object, _library, _order, cmd) = link_fixture("key");
        let first = link_key(&cmd, &output).unwrap();
        assert_eq!(first, link_key(&cmd, &output).unwrap());
        fs::write(append(&output, ".sha1"), "other").unwrap();
        // (the output's stamp is not an input)
        assert_eq!(first, link_key(&cmd, &output).unwrap());
        let _ = fs::remove_dir_all(folder);
    }
}
