//! The standalone host half of the build: compile the native runtime, opus and
//! zstd, then link every game object into the `hookshot` executable. Ported
//! from nx2/build.py's standalone Linux path. Windows needs build.py's other
//! source list and llvm-mingw link libraries, which this engine does not have
//! yet: `link_json` says so instead of guessing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::platform::Platform;

fn sources(folder: &Path, keep: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && keep(&name) {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

fn headers(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            headers(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("h") {
            out.push(path);
        }
    }
}

fn header_blob(paths: &[PathBuf], target: &Path) -> Result<(), String> {
    let mut data = Vec::new();
    for path in paths {
        data.extend(fs::read(path).unwrap_or_default());
        data.push(0);
    }
    fs::write(target, data).map_err(|e| format!("{}: {e}", target.display()))
}

fn pkg_config(names: &[&str]) -> (Vec<String>, Vec<String>) {
    let query = |flag: &str| -> Vec<String> {
        Command::new("pkg-config")
            .arg(flag)
            .args(names)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    (query("--cflags"), query("--libs"))
}

fn job(cc: &str, src: &Path, obj: &Path, flags: &[String], deps: &Path, cache: &Path) -> Value {
    json!({
        "cc": cc,
        "src": src.to_string_lossy(),
        "obj": obj.to_string_lossy(),
        "cache": cache.to_string_lossy(),
        "flags": flags,
        "deps_file": deps.to_string_lossy(),
    })
}

/// Why the host half of the build stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostBuildError {
    /// nx2/build.py's Windows branch (its source list, llvm-mingw target and
    /// link libraries) is not ported to this engine yet.
    WindowsNotPorted,
    /// The Linux build could not run, or the compiler/linker rejected it.
    Build(String),
}

impl std::fmt::Display for HostBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowsNotPorted => formatter.write_str(
                "Windows host linking is not ported to the Rust engine yet: use the Python builder (python -I nx2/build.py --standalone) on Windows, or build the host on Linux.",
            ),
            Self::Build(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for HostBuildError {}

impl From<HostBuildError> for String {
    fn from(error: HostBuildError) -> Self {
        error.to_string()
    }
}

/// Compile the host runtime and link `out/bin/<platform executable>`.
pub fn link_json(exefs: &str, work: &str, out: &str) -> Result<Value, HostBuildError> {
    link_json_on(Platform::current(), exefs, work, out)
}

fn link_json_on(
    platform: Platform,
    exefs: &str,
    work: &str,
    out: &str,
) -> Result<Value, HostBuildError> {
    match platform {
        Platform::Windows => Err(HostBuildError::WindowsNotPorted),
        Platform::Linux => link_linux_json(exefs, work, out).map_err(HostBuildError::Build),
    }
}

/// The Linux standalone host build: the source list and link line from
/// nx2/build.py's LINUX branch.
fn link_linux_json(exefs: &str, work: &str, out: &str) -> Result<Value, String> {
    let root = crate::kit::root();
    let work = Path::new(work);
    let out = Path::new(out);
    let exefs = Path::new(exefs);
    if !exefs.join("main").is_file() {
        return Err(format!("{} has no main module", exefs.display()));
    }
    let host = root.join("host");
    let posix = host.join("posix");
    let opus = root.join("third_party/opus");
    let zstd = root.join("third_party/zstd/lib");
    let rt64 = work.join("rt64");
    let game_gen = work.join("sa/gen");
    let gen_dir = work.join("build/gen");
    let obj_dir = work.join("build/obj");
    fs::create_dir_all(&obj_dir).map_err(|e| format!("{}: {e}", obj_dir.display()))?;
    let cc = crate::toolchain::find("clang", false)
        .to_string_lossy()
        .into_owned();
    let cpu = [
        format!("-march={}", crate::host::march()),
        "-ffp-contract=off".to_owned(),
        "-mcx16".to_owned(),
    ];
    let (ffmpeg_flags, ffmpeg_libs) =
        pkg_config(&["libavcodec", "libswscale", "libswresample", "libavutil"]);
    let mut deps = Vec::new();
    for folder in [
        &rt64,
        &game_gen,
        &gen_dir,
        &host,
        &posix,
        &root.join("gpu/vk"),
        &zstd,
        &opus.join("include"),
    ] {
        headers(folder, &mut deps);
    }
    let deps_file = work.join("build/host_deps.bin");
    header_blob(&deps, &deps_file)?;
    let host_flags: Vec<String> = cpu
        .iter()
        .cloned()
        .chain([
            format!("-I{}", posix.display()),
            "-D_GNU_SOURCE".into(),
            format!("-I{}", host.display()),
            format!("-I{}", rt64.display()),
            format!("-I{}", game_gen.display()),
            format!("-I{}", gen_dir.display()),
            format!("-I{}", zstd.display()),
            "-Wno-everything".into(),
            "-O2".into(),
            "-g0".into(),
            "-DA64_POLLING".into(),
            "-DA64_WRITE_TRACK".into(),
            "-D_GNU_SOURCE".into(),
        ])
        .chain(ffmpeg_flags.iter().cloned())
        .chain([format!("-I{}", opus.join("include").display())])
        .collect();
    let mut jobs = Vec::new();
    for src in sources(&host, |name| {
        name.ends_with(".c")
            && name != "pgo_rt.c"
            && name != "display.c"
            && name != "mf_av1.c"
            && name != "rx_nowgpu.c"
    })? {
        let obj = obj_dir.join(format!(
            "host_{}.o",
            src.file_stem().unwrap().to_string_lossy()
        ));
        let mut flags = host_flags.clone();
        if src.file_stem().and_then(|name| name.to_str()) == Some("hos_main") {
            flags.push(crate::version::define(&root));
        }
        jobs.push(job(
            &cc,
            &src,
            &obj,
            &flags,
            &deps_file,
            &work.join("build/objcache"),
        ));
    }
    for src in sources(&posix, |name| {
        matches!(
            name,
            "win32_posix.c" | "display_sdl.c" | "linux_stubs.c" | "prof.c"
        )
    })? {
        let obj = obj_dir.join(format!(
            "host_{}.o",
            src.file_stem().unwrap().to_string_lossy()
        ));
        let mut flags = host_flags.clone();
        flags.extend([format!("-I{}", posix.display()), "-D_GNU_SOURCE".into()]);
        jobs.push(job(
            &cc,
            &src,
            &obj,
            &flags,
            &deps_file,
            &work.join("build/objcache"),
        ));
    }
    // (build.py puts the version on hos_main and the repository on hos_update)
    if let Some(pos) = jobs.iter().position(|job| {
        job["obj"]
            .as_str()
            .is_some_and(|name| name.ends_with("host_hos_update.o"))
    }) {
        let flags = jobs[pos]["flags"].as_array().cloned().unwrap_or_default();
        let mut flags: Vec<String> = flags
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        flags.push(format!(
            "-DHOOKSHOT_REPOSITORY=\"{}\"",
            crate::version::repository(&root)
        ));
        jobs[pos]["flags"] = json!(flags);
    }
    let opus_flags: Vec<String> = cpu
        .iter()
        .cloned()
        .chain([
            "-O2".into(),
            "-g0".into(),
            "-Wno-everything".into(),
            "-DOPUS_BUILD".into(),
            "-DUSE_ALLOCA".into(),
            "-DHAVE_LRINTF".into(),
            "-DHAVE_LRINT".into(),
        ])
        .chain(
            ["include", "celt", "silk", "silk/float", "src"]
                .iter()
                .map(|dir| format!("-I{}", opus.join(dir).display())),
        )
        .collect();
    let opus_list = fs::read_to_string(opus.join("sources.txt")).map_err(|e| e.to_string())?;
    for rel in opus_list.split_whitespace() {
        let src = opus.join(rel);
        let obj = obj_dir.join(format!(
            "opus_{}.o",
            rel.replace('/', "_").trim_end_matches(".c")
        ));
        jobs.push(job(
            &cc,
            &src,
            &obj,
            &opus_flags,
            &deps_file,
            &work.join("build/objcache"),
        ));
    }
    let zstd_flags: Vec<String> = cpu
        .iter()
        .cloned()
        .chain([
            "-O2".into(),
            "-g0".into(),
            "-DNDEBUG".into(),
            "-Wno-everything".into(),
            "-DZSTD_DISABLE_ASM".into(),
            "-DZSTD_LEGACY_SUPPORT=0".into(),
            "-DZSTD_MULTITHREAD=0".into(),
        ])
        .collect();
    for rel in [
        "common/debug.c",
        "common/entropy_common.c",
        "common/error_private.c",
        "common/fse_decompress.c",
        "common/xxhash.c",
        "common/zstd_common.c",
        "decompress/huf_decompress.c",
        "decompress/zstd_ddict.c",
        "decompress/zstd_decompress.c",
        "decompress/zstd_decompress_block.c",
    ] {
        let src = zstd.join(rel);
        let obj = obj_dir.join(format!(
            "zstd_{}.o",
            Path::new(rel).file_stem().unwrap().to_string_lossy()
        ));
        jobs.push(job(
            &cc,
            &src,
            &obj,
            &zstd_flags,
            &deps_file,
            &work.join("build/objcache"),
        ));
    }
    for name in ["a64_stubs.c", "hos_image.c", "nvn2_exports.c"] {
        let src = game_gen.join(name);
        let already = work.join("obj").join(format!("{name}.o"));
        if src.is_file() && !already.is_file() {
            let obj = obj_dir.join(format!(
                "gen_{}.o",
                Path::new(name).file_stem().unwrap().to_string_lossy()
            ));
            jobs.push(job(
                &cc,
                &src,
                &obj,
                &host_flags,
                &deps_file,
                &work.join("build/objcache"),
            ));
        }
    }
    let compiled = crate::compile::many_json(
        &json!({"jobs": jobs, "cache": work.join("build/objcache"), "parallel": 8}),
        &root,
    )?;
    if compiled["failed"]
        .as_array()
        .is_some_and(|failed| !failed.is_empty())
    {
        return Ok(json!({"stage": "host-compile", "compiled": compiled}));
    }
    let mut objects: Vec<String> = fs::read_dir(work.join("obj"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    entry.path().is_file()
                        && entry.path().extension().and_then(|ext| ext.to_str()) == Some("o")
                })
                .map(|entry| entry.path().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    for entry in fs::read_dir(&obj_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("o") {
            continue;
        }
        if let Some(stem) = name
            .strip_prefix("gen_")
            .and_then(|name| name.strip_suffix(".o"))
        {
            if work.join("obj").join(format!("{stem}.c.o")).is_file() {
                continue;
            }
        }
        objects.push(entry.path().to_string_lossy().into_owned());
    }
    objects.sort();
    let bin = out.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let exe = bin.join(Platform::Linux.executable());
    let mut args = vec![
        "-fuse-ld=lld".to_owned(),
        "-o".to_owned(),
        exe.to_string_lossy().into_owned(),
        "-Wl,--gc-sections".to_owned(),
        "-Wl,--export-dynamic-symbol=hos_posix_*".to_owned(),
        "-Wl,--no-as-needed".to_owned(),
    ];
    let ffmpeg_linked = !ffmpeg_libs.is_empty();
    args.extend(ffmpeg_libs);
    args.extend(["-lpthread".into(), "-ldl".into(), "-lm".into()]);
    let link = crate::linker::link_json(&json!({
        "cc": cc,
        "out": exe.to_string_lossy(),
        "objects": objects,
        "args": args,
    }))?;
    let size = fs::metadata(&exe).map(|meta| meta.len()).unwrap_or(0);
    Ok(json!({
        "stage": if link["ok"].as_bool().unwrap_or(false) { "host-linked" } else { "host-link" },
        "compiled": compiled,
        "link": link,
        "exe": exe.to_string_lossy(),
        "size": size,
        "ffmpeg": ffmpeg_linked,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_host_link_is_reported_as_not_ported() {
        let error = link_json_on(Platform::Windows, "", "", "").unwrap_err();
        assert_eq!(error, HostBuildError::WindowsNotPorted);
        let message = error.to_string();
        assert!(message.contains("not ported"), "{message}");
        assert!(message.contains("nx2/build.py"), "{message}");
    }

    #[test]
    fn linux_host_link_still_checks_its_inputs() {
        let error = link_json_on(Platform::Linux, "", "", "").unwrap_err();
        assert!(matches!(error, HostBuildError::Build(_)));
        assert!(error.to_string().contains("has no main module"));
    }
}
