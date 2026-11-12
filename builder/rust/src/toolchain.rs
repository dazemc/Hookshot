//! Compiler discovery and target flags. Ported from a64-kit's toolchain.py.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

pub fn emsdk() -> PathBuf {
    std::env::var_os("EMSDK")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                PathBuf::from(r"C:\emsdk")
            } else {
                home().join("emsdk")
            }
        })
}

/// The game is built for Windows: on Windows, or with RX_TARGET=windows on
/// another system.
pub fn windows() -> bool {
    cfg!(windows)
        || std::env::var("RX_TARGET")
            .map(|value| value.eq_ignore_ascii_case("windows"))
            .unwrap_or(false)
}

/// HOOKSHOT_MINGW=<llvm-mingw folder>: Windows builds with that toolchain.
/// Empty: the MSVC target. `host`: a tool the build itself runs, which a
/// cross build makes for this PC.
pub fn mingw(host: bool) -> Option<PathBuf> {
    if windows() && !(host && !cfg!(windows)) {
        std::env::var_os("HOOKSHOT_MINGW")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    } else {
        None
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for folder in std::env::split_paths(&path) {
        let candidate = folder.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cfg!(windows) {
            for extension in [".exe", ".bat"] {
                let candidate = folder.join(format!("{name}{extension}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// A target-prefixed clang on PATH: a cross build's llvm-mingw that was not
/// named by HOOKSHOT_MINGW. Its wrapper knows the MinGW sysroot.
fn mingw_on_path() -> Option<PathBuf> {
    if cfg!(windows) || !windows() {
        return None;
    }
    which("x86_64-w64-mingw32-clang").or_else(|| which("x86_64-w64-mingw32-clang++"))
}

fn mingw_build(host: bool) -> bool {
    mingw(host).is_some() || (!host && mingw_on_path().is_some())
}

pub fn find(tool: &str, host: bool) -> PathBuf {
    if let Some(mingw) = mingw(host) {
        if tool == "clang" || tool == "clang++" || tool == "lld" || tool.starts_with("llvm-") {
            let name = if tool == "lld" { "ld.lld" } else { tool };
            let path = mingw
                .join("bin")
                .join(format!("{name}{}", if cfg!(windows) { ".exe" } else { "" }));
            if path.is_file() {
                return path;
            }
        }
    }
    // (a cross build on Linux whose llvm-mingw is on PATH rather than named by
    // HOOKSHOT_MINGW: its wrapper knows the sysroot the target needs)
    if !host && !cfg!(windows) && windows() {
        let wrapper = match tool {
            "clang" => Some("x86_64-w64-mingw32-clang"),
            "clang++" => Some("x86_64-w64-mingw32-clang++"),
            _ => None,
        };
        if let Some(wrapper) = wrapper
            && let Some(path) = which(wrapper)
        {
            return path;
        }
    }
    let native = tool == "clang" || tool == "clang++" || tool == "lld" || tool.starts_with("llvm-");
    let folder = if native {
        emsdk().join("upstream/bin")
    } else {
        emsdk().join("upstream/emscripten")
    };
    for extension in if cfg!(windows) {
        &[".exe", ".bat"][..]
    } else {
        &[""][..]
    } {
        let path = folder.join(format!("{tool}{extension}"));
        if path.is_file() {
            return path;
        }
    }
    which(tool).unwrap_or_else(|| {
        folder.join(format!("{tool}{}", if cfg!(windows) { ".exe" } else { "" }))
    })
}

pub fn need(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err(format!(
            "{} not found: set EMSDK to an emsdk checkout (now {}), or put {} on PATH",
            path.display(),
            emsdk().display(),
            path.file_stem()
                .map(|name| name.to_string_lossy())
                .unwrap_or_default()
        ))
    }
}

/// `--target=...` and the Windows extensions the code takes for granted.
pub fn native_target(host: bool) -> Vec<String> {
    if mingw_build(host) {
        return [
            "--target=x86_64-w64-mingw32",
            "-fms-extensions",
            "-D_CRT_SECURE_NO_WARNINGS",
        ]
        .iter()
        .map(|flag| (*flag).to_owned())
        .collect();
    }
    if cfg!(windows) {
        return [
            "--target=x86_64-pc-windows-msvc",
            "-fms-extensions",
            "-D_CRT_SECURE_NO_WARNINGS",
        ]
        .iter()
        .map(|flag| (*flag).to_owned())
        .collect();
    }
    Vec::new()
}

pub fn native_link(host: bool) -> Vec<String> {
    if mingw_build(host) {
        return [
            "-fuse-ld=lld",
            "-static",
            "-Wl,-Xlink=/nodefaultlib:libcmt",
            "-Wl,-Xlink=/nodefaultlib:oldnames",
            "-Wl,-Xlink=/nodefaultlib:libvcruntime",
            "-Wl,-Xlink=/nodefaultlib:msvcrt",
        ]
        .iter()
        .map(|flag| (*flag).to_owned())
        .collect();
    }
    if cfg!(windows) {
        let lld = find("lld", false);
        let folder = lld
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_default();
        return vec![
            "-fuse-ld=lld".to_owned(),
            format!("-B{}", folder.display()),
            "-Wl,/nodefaultlib:oldnames".to_owned(),
        ];
    }
    Vec::new()
}

fn physical_memory() -> Option<u64> {
    if cfg!(windows) {
        return windows_memory();
    }
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    Some(line.split_whitespace().nth(1)?.parse::<u64>().ok()? * 1024)
}

#[cfg(windows)]
fn windows_memory() -> Option<u64> {
    #[repr(C)]
    struct Status {
        length: u32,
        load: u32,
        total: u64,
        available: u64,
        page: u64,
        avail_page: u64,
        virt: u64,
        avail_virt: u64,
        avail_ext: u64,
    }
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(status: *mut Status) -> i32;
    }
    let mut status = Status {
        length: std::mem::size_of::<Status>() as u32,
        load: 0,
        total: 0,
        available: 0,
        page: 0,
        avail_page: 0,
        virt: 0,
        avail_virt: 0,
        avail_ext: 0,
    };
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then_some(status.total)
}

#[cfg(not(windows))]
fn windows_memory() -> Option<u64> {
    None
}

/// Compiles that fit in memory: 80% of the physical memory at 3.5 GB each,
/// at most one a core. RX_JOBS sets it.
pub fn default_jobs(gb_per_job: f64) -> usize {
    if let Ok(value) = std::env::var("RX_JOBS") {
        if let Ok(jobs) = value.parse::<usize>() {
            return jobs.max(1);
        }
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let Some(total) = physical_memory() else {
        return cores;
    };
    let fits = (total as f64 / (1u64 << 30) as f64 * 0.8 / gb_per_job) as usize;
    cores.min(fits).max(1)
}

pub fn json() -> Value {
    let clang = find("clang", false);
    let mingw = mingw(false).or_else(|| {
        let wrapper = mingw_on_path()?;
        wrapper.parent()?.parent().map(Path::to_path_buf)
    });
    json!({
        "windows": windows(),
        "linux": !windows(),
        "mingw": mingw.map(|path| path.to_string_lossy().into_owned()),
        "target": native_target(false),
        "link": native_link(false),
        "clang": clang.to_string_lossy(),
        "clang_ready": need(&clang).is_ok(),
        "clang_host": find("clang", true).to_string_lossy(),
        "default_jobs": default_jobs(3.5),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_matches_the_host() {
        let target = native_target(false);
        if cfg!(windows) || windows() {
            assert!(target.iter().any(|flag| flag.starts_with("--target=")));
        } else {
            assert!(target.is_empty());
        }
    }

    #[test]
    fn paths_are_found() {
        let cc = find(if cfg!(windows) { "clang" } else { "cc" }, true);
        assert!(!cc.as_os_str().is_empty());
        assert!(need(&cc).is_ok());
    }
}
