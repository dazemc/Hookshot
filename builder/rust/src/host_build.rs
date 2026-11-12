//! The standalone host half of the build: compile the native runtime, opus and
//! zstd, then link every game object into the `hookshot` executable. Ported
//! from nx2/build.py's standalone Linux and Windows (llvm-mingw) paths.
//!
//! Windows adds host/display.c, host/mf_av1.c, host/dav1d_av1.c (when the CPM cache has dav1d), host/pad_sdl.c,
//! host/mingw_msvc_shims.c and the other sources build.py only compiles there,
//! links Microsoft's Win32 libraries, and puts the manifest, the icon and the
//! crash map beside the executable. The WebGPU renderer is compiled in when a
//! wgpu-native release is there, as the Python builder does. NVIDIA's NGX SDK
//! and AMD's FidelityFX headers are reported as inputs but belong to
//! hookshot_vk.dll, which this engine does not link yet.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::platform::Platform;

const WINDOWS_TARGET: &str = "x86_64-w64-mingw32";
const FFMPEG_LIBRARIES: [&str; 4] = ["avcodec", "swscale", "swresample", "avutil"];
/// nx2/build.py's HOST_LIBS for Windows, in its order.
const WINDOWS_HOST_LIBRARIES: [&str; 8] = [
    "-lwinhttp",
    "-luser32",
    "-lwinmm",
    "-ladvapi32",
    "-lole32",
    "-lshell32",
    "-lgdi32",
    "-ldbghelp",
];

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

fn write_blob(path: &Path, data: &[u8]) -> Result<(), String> {
    fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))
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

fn opus_jobs(
    cc: &str,
    root: &Path,
    obj_dir: &Path,
    deps: &Path,
    cache: &Path,
    base: &[String],
) -> Result<Vec<Value>, String> {
    let opus = root.join("third_party/opus");
    let mut flags: Vec<String> = base.to_vec();
    flags.extend([
        "-O2".into(),
        "-g0".into(),
        "-Wno-everything".into(),
        "-DOPUS_BUILD".into(),
        "-DUSE_ALLOCA".into(),
        "-DHAVE_LRINTF".into(),
        "-DHAVE_LRINT".into(),
    ]);
    flags.extend(
        ["include", "celt", "silk", "silk/float", "src"]
            .iter()
            .map(|dir| format!("-I{}", opus.join(dir).display())),
    );
    let list = fs::read_to_string(opus.join("sources.txt"))
        .map_err(|e| format!("{}: {e}", opus.join("sources.txt").display()))?;
    let mut jobs = Vec::new();
    for rel in list.split_whitespace() {
        let src = opus.join(rel);
        let obj = obj_dir.join(format!(
            "opus_{}.o",
            rel.replace('/', "_").trim_end_matches(".c")
        ));
        jobs.push(job(cc, &src, &obj, &flags, deps, cache));
    }
    Ok(jobs)
}

fn zstd_jobs(
    cc: &str,
    root: &Path,
    obj_dir: &Path,
    deps: &Path,
    cache: &Path,
    base: &[String],
) -> Vec<Value> {
    let zstd = root.join("third_party/zstd/lib");
    let mut flags: Vec<String> = base.to_vec();
    flags.extend([
        "-O2".into(),
        "-g0".into(),
        "-DNDEBUG".into(),
        "-Wno-everything".into(),
        "-DZSTD_DISABLE_ASM".into(),
        "-DZSTD_LEGACY_SUPPORT=0".into(),
        "-DZSTD_MULTITHREAD=0".into(),
    ]);
    let mut jobs = Vec::new();
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
        jobs.push(job(cc, &src, &obj, &flags, deps, cache));
    }
    jobs
}

fn collect_objects(work: &Path, job_objects: &[String]) -> Result<Vec<String>, String> {
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
    for object in job_objects {
        let path = Path::new(object);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(stem) = name
            .strip_prefix("gen_")
            .and_then(|name| name.strip_suffix(".o"))
            && work.join("obj").join(format!("{stem}.c.o")).is_file()
        {
            // (the driver already compiled this generated file with the
            // lifted code's flags; link that one object once)
            continue;
        }
        objects.push(object.clone());
    }
    objects.sort();
    objects.dedup();
    Ok(objects)
}

/// Why the host half of the build stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostBuildError {
    /// No llvm-mingw that can link for x86_64-w64-mingw32 was found.
    NoWindowsToolchain(String),
    /// The build could not run, or the compiler/linker rejected it.
    Build(String),
}

impl std::fmt::Display for HostBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowsToolchain(message) | Self::Build(message) => {
                formatter.write_str(message)
            }
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
        Platform::Windows => link_windows_json(exefs, work, out),
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
    let mut cpu = crate::host::cpu_flags();
    cpu.extend([
        // (no contraction: a C expression is rounded as written, as the
        // lifted code's exact bodies assume)
        "-ffp-contract=off".to_owned(),
        "-mcx16".to_owned(),
    ]);
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
            && name != "dav1d_av1.c"
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
    jobs.extend(opus_jobs(
        &cc,
        &root,
        &obj_dir,
        &deps_file,
        &work.join("build/objcache"),
        &cpu,
    )?);
    jobs.extend(zstd_jobs(
        &cc,
        &root,
        &obj_dir,
        &deps_file,
        &work.join("build/objcache"),
        &cpu,
    ));
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
    let job_objects: Vec<String> = jobs
        .iter()
        .filter_map(|job| job["obj"].as_str().map(str::to_owned))
        .collect();
    let _ = crate::compile::write_compdb(&work.join("build").join(crate::compile::COMPDB), &jobs);
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
    let objects = collect_objects(work, &job_objects)?;
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

/// One llvm-mingw checkout: the C driver, and the C++ one when it has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MingwToolchain {
    pub clang: PathBuf,
    pub clangxx: Option<PathBuf>,
}

fn tool_in(bin: &Path, names: &[&str]) -> Option<PathBuf> {
    for name in names {
        for candidate in [bin.join(name), bin.join(format!("{name}.exe"))] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// `HOOKSHOT_MINGW`'s folder, as llvm-mingw lays it out: `bin/clang` (the
/// generic driver) or the target-prefixed wrapper beside it.
fn toolchain_in_folder(folder: &Path) -> Option<MingwToolchain> {
    let bin = folder.join("bin");
    let clang = tool_in(
        &bin,
        &[
            "clang",
            "x86_64-w64-mingw32-clang",
            "clang++",
            "x86_64-w64-mingw32-clang++",
        ],
    )?;
    let clangxx =
        tool_in(&bin, &["clang++", "x86_64-w64-mingw32-clang++"]).or_else(|| Some(clang.clone()));
    Some(MingwToolchain { clang, clangxx })
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for folder in std::env::split_paths(&path) {
        for candidate in [folder.join(name), folder.join(format!("{name}.exe"))] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// A plain `clang` is only a Windows toolchain if it can link a trivial
/// program for the target: `--target=x86_64-w64-mingw32` alone compiles
/// objects with this PC's GCC driver, then fails on the missing MinGW runtime.
fn probe_mingw(clang: &Path) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0);
    let folder = std::env::temp_dir().join(format!(
        "hookshot-rust-mingw-probe-{}-{stamp}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let source = folder.join("probe.c");
    let exe = folder.join("probe.exe");
    fs::write(&source, "int main(void) { return 0; }\n")
        .map_err(|e| format!("{}: {e}", source.display()))?;
    let output = Command::new(clang)
        .args(["--target=x86_64-w64-mingw32", "-fuse-ld=lld"])
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .map_err(|e| format!("{}: {e}", clang.display()))?;
    let ok = output.status.success() && exe.is_file();
    let _ = fs::remove_dir_all(&folder);
    if ok {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("the link did not finish")
        .trim();
    Err(line.chars().take(180).collect())
}

/// The llvm-mingw to build with: HOOKSHOT_MINGW, then a target-prefixed clang
/// on PATH, then a plain clang that can link for the target.
pub(crate) fn find_windows_toolchain() -> Result<MingwToolchain, String> {
    let mut tried = Vec::new();
    if let Some(folder) = std::env::var_os("HOOKSHOT_MINGW").filter(|value| !value.is_empty()) {
        let folder = PathBuf::from(folder);
        if let Some(toolchain) = toolchain_in_folder(&folder) {
            return Ok(toolchain);
        }
        tried.push(format!(
            "HOOKSHOT_MINGW={} has no bin/clang",
            folder.display()
        ));
    }
    if let Some(clang) =
        on_path("x86_64-w64-mingw32-clang").or_else(|| on_path("x86_64-w64-mingw32-clang++"))
    {
        let clangxx = on_path("x86_64-w64-mingw32-clang++")
            .or_else(|| on_path("clang++"))
            .filter(|path| path.is_file());
        return Ok(MingwToolchain { clang, clangxx });
    }
    tried.push("neither x86_64-w64-mingw32-clang nor x86_64-w64-mingw32-clang++ is on PATH".into());
    let clang = crate::toolchain::find("clang", false);
    if clang.is_file() {
        match probe_mingw(&clang) {
            Ok(()) => {
                let clangxx = crate::toolchain::find("clang++", false);
                return Ok(MingwToolchain {
                    clang,
                    clangxx: clangxx.is_file().then_some(clangxx),
                });
            }
            Err(error) => tried.push(format!(
                "{} cannot link for the target ({error})",
                clang.display()
            )),
        }
    } else {
        tried.push("no clang on PATH".into());
    }
    Err(format!(
        "the Windows build needs llvm-mingw: set HOOKSHOT_MINGW to an llvm-mingw folder (its bin/clang), or put x86_64-w64-mingw32-clang on PATH (on Linux, build with RX_TARGET=windows). The Visual Studio route is not ported to this engine. {}",
        tried.join("; ")
    ))
}

pub(crate) fn mingw_target_flags() -> Vec<String> {
    [
        format!("--target={WINDOWS_TARGET}"),
        "-fms-extensions".to_owned(),
        "-D_CRT_SECURE_NO_WARNINGS".to_owned(),
    ]
    .to_vec()
}

/// nx2/toolchain.py's native_link() for llvm-mingw: lld, the static runtime,
/// and no Microsoft C runtime libraries in an FFmpeg or NGX object's way.
pub(crate) fn mingw_link_flags() -> Vec<String> {
    [
        "-fuse-ld=lld",
        "-static",
        "-Wl,-Xlink=/nodefaultlib:libcmt",
        "-Wl,-Xlink=/nodefaultlib:oldnames",
        "-Wl,-Xlink=/nodefaultlib:libvcruntime",
        "-Wl,-Xlink=/nodefaultlib:msvcrt",
    ]
    .iter()
    .map(|flag| (*flag).to_owned())
    .collect()
}

/// The target, the PC's CPU level and no contraction, as the lifted code's
/// exact bodies expect; the runtime, opus and zstd share it.
fn mingw_base_flags() -> Vec<String> {
    let mut flags = mingw_target_flags();
    flags.extend(crate::host::cpu_flags());
    flags.push("-ffp-contract=off".into());
    flags
}

fn windows_host_flags(
    root: &Path,
    work: &Path,
    ffmpeg: Option<&Path>,
    dav1d: Option<&Path>,
) -> Vec<String> {
    let mut flags = mingw_base_flags();
    flags.extend([
        format!("-I{}", root.join("host").display()),
        format!("-I{}", work.join("rt64").display()),
        format!("-I{}", work.join("sa/gen").display()),
        "-Wno-everything".into(),
        "-O2".into(),
        "-g0".into(),
        "-mcx16".into(),
        "-DA64_POLLING".into(),
        "-DA64_WRITE_TRACK".into(),
    ]);
    if let Some(ffmpeg) = ffmpeg {
        flags.push(format!("-I{}", ffmpeg.join("include").display()));
    }
    if let Some(dav1d) = dav1d {
        flags.push("-DHOOKSHOT_DAV1D".into());
        flags.push(format!("-I{}", dav1d.join("include").display()));
    }
    flags
}

fn windows_host_sources(
    root: &Path,
    work: &Path,
    wgpu: bool,
    pgo_gen: bool,
    dav1d: bool,
) -> Result<Vec<PathBuf>, String> {
    let host = root.join("host");
    let mut out = sources(&host, |name| {
        name.ends_with(".c")
            && (name != "pgo_rt.c" || pgo_gen)
            && (name != "rx_nowgpu.c" || !wgpu)
            && (name != "dav1d_av1.c" || dav1d)
    })?;
    for name in ["a64_stubs.c", "hos_image.c", "nvn2_exports.c"] {
        let path = work.join("sa/gen").join(name);
        if path.is_file() {
            out.push(path);
        }
    }
    Ok(out)
}

fn cpm_dirs(root: &Path, work: &Path) -> Vec<PathBuf> {
    let parent = root.parent().unwrap_or(root);
    [
        std::env::var_os("RX_CPM").map(PathBuf::from),
        Some(root.join("cpm")),
        Some(work.join("eg/.cache/cpm")),
        Some(parent.join("mkw-recomp/work/eg/.cache/cpm")),
        Some(parent.join("Eglantine/.cache/cpm")),
    ]
    .into_iter()
    .flatten()
    .filter(|path| path.is_dir())
    .collect()
}

/// The Windows FFmpeg in a CPM cache (build.py skips a package named for
/// Linux: a cache can hold both systems' builds).
fn find_ffmpeg_in(caches: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for cache in caches {
        let folder = cache.join("ffmpeg");
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if name.to_lowercase().contains("linux") {
                continue;
            }
            let package = folder.join(&name);
            if package.join("lib").is_dir() {
                return Some(package);
            }
        }
    }
    None
}

fn find_ffmpeg(root: &Path, work: &Path) -> Option<PathBuf> {
    find_ffmpeg_in(cpm_dirs(root, work))
}

/// dav1d in a CPM cache (scripts/windows/dav1d.sh): the Windows AV1
/// decoder host/dav1d_av1.c links when the CPM FFmpeg has none.
fn find_dav1d_in(caches: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for cache in caches {
        let folder = cache.join("dav1d");
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names.into_iter().rev() {
            let package = folder.join(name);
            if package.join("lib/libdav1d.a").is_file()
                && package.join("include/dav1d/dav1d.h").is_file()
            {
                return Some(package);
            }
        }
    }
    None
}

fn find_dav1d(root: &Path, work: &Path) -> Option<PathBuf> {
    find_dav1d_in(cpm_dirs(root, work))
}

/// The FFmpeg headers an object was compiled against: codec numbers and
/// struct layouts change between major versions.
fn ffmpeg_version_key(ffmpeg: Option<&Path>) -> Vec<u8> {
    let mut include = Vec::new();
    if let Some(ffmpeg) = ffmpeg {
        include.push(ffmpeg.join("include"));
    }
    include.push(PathBuf::from("/usr/include"));
    for folder in include {
        let mut parts = Vec::new();
        for name in ["version_major.h", "version.h"] {
            if let Ok(data) = fs::read(folder.join("libavcodec").join(name)) {
                parts.extend(data);
            }
        }
        if !parts.is_empty() {
            return parts;
        }
    }
    Vec::new()
}

fn top_headers(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("h") {
            out.push(path);
        }
    }
}

fn windows_host_deps(
    root: &Path,
    work: &Path,
    ffmpeg: Option<&Path>,
    dav1d: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let mut paths = Vec::new();
    for folder in [
        work.join("rt64"),
        work.join("sa/gen"),
        root.join("host"),
        root.join("third_party/opus/include"),
    ] {
        top_headers(&folder, &mut paths);
    }
    let mut deps = Vec::new();
    for path in paths {
        deps.extend(fs::read(path).unwrap_or_default());
        deps.push(0);
    }
    for file in [
        root.join("gpu/vk/dlss_math.h"),
        root.join("gpu/vk/compat/hookshot_env.h"),
        root.join("gpu/vk/vk_api.h"),
        root.join("gpu/vk/oot_cheat_items.h"),
        root.join("gpu/render/rx.h"),
        root.join("gpu/render/rx_nvn.h"),
        root.join("gpu/render/nvn_api.h"),
    ] {
        if let Ok(data) = fs::read(&file) {
            deps.extend(data);
            deps.push(0);
        }
    }
    deps.extend(ffmpeg_version_key(ffmpeg));
    if let Some(dav1d) = dav1d {
        if let Ok(data) = fs::read(dav1d.join("include/dav1d/version.h")) {
            deps.extend(data);
            deps.push(0);
        }
    }
    Ok(deps)
}

fn zstd_api_headers(root: &Path) -> Vec<u8> {
    let zstd = root.join("third_party/zstd/lib");
    let mut data = Vec::new();
    let Ok(entries) = fs::read_dir(&zstd) else {
        return data;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("h"))
        .collect();
    paths.sort();
    for path in paths {
        data.extend(fs::read(path).unwrap_or_default());
        data.push(0);
    }
    data
}

/// gpu/render and the suyu sources build.py compiles into the Windows
/// executable when a wgpu-native release is there.
fn gpu_sources(gpu: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let suyu = gpu.join("third_party/suyu/shader_recompiler");
    let mut stack = vec![suyu];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("glsl") {
                    stack.push(path);
                }
            } else {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if name.ends_with(".cpp") && name != "attribute.cpp" && name != "flow_test.cpp" {
                    out.push(path);
                }
            }
        }
    }
    let video = gpu.join("third_party/suyu/video_core");
    for rel in [
        "surface.cpp",
        "texture_cache/format_lookup_table.cpp",
        "textures/astc.cpp",
        "textures/decoders.cpp",
    ] {
        let path = video.join(rel);
        if path.is_file() {
            out.push(path);
        }
    }
    for entry in fs::read_dir(gpu.join("render")).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("cpp") {
            out.push(entry.path());
        }
    }
    out.sort();
    Ok(out)
}

fn tree_headers(roots: &[PathBuf], skip: &[&str]) -> Vec<u8> {
    let mut files = Vec::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(folder) = stack.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if !skip.contains(&name) {
                    stack.push(path);
                }
            } else if matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("h" | "hpp" | "inc")
            ) {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut data = Vec::new();
    for path in files {
        data.extend(
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into_bytes(),
        );
        data.extend(fs::read(path).unwrap_or_default());
    }
    data
}

fn wgpu_native_folder() -> PathBuf {
    std::env::var_os("WGPU_NATIVE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Desktop/TOTKPort/third_party/wgpu-native")
        })
}

fn wgpu_header(folder: &Path) -> Option<PathBuf> {
    let header = folder.join("include/webgpu/webgpu.h");
    header.is_file().then_some(header)
}

fn ngx_windows_library() -> Option<PathBuf> {
    let sdk = std::env::var_os("HOOKSHOT_NGX_SDK").filter(|value| !value.is_empty())?;
    let path = crate::ngx_sdk::library(Path::new(&sdk), crate::ngx_sdk::Platform::Windows);
    path.is_file().then_some(path)
}

fn fsr_headers(root: &Path) -> Option<PathBuf> {
    (std::env::var("HOOKSHOT_FSR").ok().as_deref() == Some("1"))
        .then(|| root.join("third_party/fidelityfx"))
        .filter(|path| path.is_dir())
}

/// The optional pieces build.py finds before it links the Windows
/// executable. The NGX and FidelityFX entries belong to hookshot_vk.dll and
/// are reported only.
#[derive(Clone, Debug, Default)]
struct WindowsOptions {
    toolchain: Option<MingwToolchain>,
    toolchain_error: Option<String>,
    ffmpeg: Option<PathBuf>,
    dav1d: Option<PathBuf>,
    wgpu: Option<PathBuf>,
    sdl: Option<PathBuf>,
    ngx: Option<PathBuf>,
    fsr: Option<PathBuf>,
    hot_order: Option<PathBuf>,
    release: bool,
    pgo_gen: bool,
}

fn windows_options(root: &Path, work: &Path) -> WindowsOptions {
    let (toolchain, toolchain_error) = match find_windows_toolchain() {
        Ok(toolchain) => (Some(toolchain), None),
        Err(error) => (None, Some(error)),
    };
    let wgpu = wgpu_native_folder();
    let wgpu = wgpu_header(&wgpu).map(|_| wgpu);
    let hot_order = work.join("hot_order.txt");
    WindowsOptions {
        toolchain,
        toolchain_error,
        ffmpeg: find_ffmpeg(root, work),
        dav1d: find_dav1d(root, work),
        wgpu,
        sdl: Some(root.join("third_party/sdl2")),
        ngx: ngx_windows_library(),
        fsr: fsr_headers(root),
        hot_order: (hot_order.is_file() && !crate::kit::env_on("RX_NO_ORDER")).then_some(hot_order),
        release: std::env::var("RX_RELEASE").ok().as_deref() == Some("1"),
        pgo_gen: std::env::var("RX_PGO").ok().as_deref() == Some("gen"),
    }
}

fn xlink(option: &str) -> [String; 2] {
    ["-Xlinker".to_owned(), format!("-Xlink={option}")]
}

/// The link line before the object response file: compiler runtime and the
/// output. The libraries follow the objects (nx2/build.py's order).
fn windows_link_prefix(exe: &Path, map: &Path) -> Vec<String> {
    let mut args = mingw_target_flags();
    args.extend(mingw_link_flags());
    args.extend(["-o".to_owned(), exe.to_string_lossy().into_owned()]);
    args.extend(xlink(&format!("/map:{}", map.display())));
    args.extend(xlink("/stack:8388608"));
    args
}

/// The link line after the objects: ordering, Win32 and C++ libraries, the
/// resource and manifest, and the optional renderer and FFmpeg inputs.
fn windows_link_suffix(
    manifest: &Path,
    resource: Option<&Path>,
    options: &WindowsOptions,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(order) = &options.hot_order {
        let order = order.to_string_lossy().replace('\\', "/");
        args.extend(xlink(&format!("/order:@{order}")));
        args.extend(xlink("/ignore:4037"));
    }
    args.extend(WINDOWS_HOST_LIBRARIES.iter().map(|lib| (*lib).to_owned()));
    if options.pgo_gen {
        args.extend(xlink("/nodefaultlib:clang_rt.profile.lib"));
    }
    if let Some(resource) = resource {
        args.push(resource.to_string_lossy().into_owned());
    }
    args.push(manifest.to_string_lossy().into_owned());
    if options.release {
        args.extend(xlink("/subsystem:windows"));
        args.extend(xlink("/entry:mainCRTStartup"));
    }
    if let Some(wgpu) = &options.wgpu {
        args.push(
            wgpu.join("lib/wgpu_native.dll.lib")
                .to_string_lossy()
                .into_owned(),
        );
    }
    if let Some(ffmpeg) = &options.ffmpeg {
        for library in FFMPEG_LIBRARIES {
            args.push(
                ffmpeg
                    .join("lib")
                    .join(format!("{library}.lib"))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        args.extend(
            ["-lbcrypt", "-lmfplat", "-lmfuuid", "-lstrmiids", "-lole32"]
                .iter()
                .map(|lib| (*lib).to_owned()),
        );
    }
    if let Some(dav1d) = &options.dav1d {
        args.push(dav1d.join("lib/libdav1d.a").to_string_lossy().into_owned());
    }
    // (llvm-mingw links the C++ runtime in: the WebGPU renderer and the
    // C++ parts of the static MSVC libraries need it)
    args.extend(["-lc++".to_owned(), "-lunwind".to_owned()]);
    args
}

fn windows_inputs(options: &WindowsOptions) -> Value {
    let ffmpeg = options.ffmpeg.as_ref().map(|ffmpeg| {
        json!({
            "dir": ffmpeg.to_string_lossy(),
            "libraries": FFMPEG_LIBRARIES,
        })
    });
    let dav1d = options
        .dav1d
        .as_ref()
        .map(|folder| folder.to_string_lossy().into_owned());
    let sdl = options
        .sdl
        .as_ref()
        .filter(|folder| folder.is_dir())
        .map(|folder| folder.to_string_lossy().into_owned());
    let wgpu = options
        .wgpu
        .as_ref()
        .map(|folder| folder.to_string_lossy().into_owned());
    let dlss = options.ngx.as_ref().map(|library| {
        json!({
            "library": library.to_string_lossy(),
            "linked_by": "hookshot_vk.dll",
        })
    });
    let fsr = options.fsr.as_ref().map(|headers| {
        json!({
            "headers": headers.to_string_lossy(),
            "compiled_by": "hookshot_vk.dll",
        })
    });
    json!({
        "ffmpeg": ffmpeg,
        "dav1d": dav1d,
        "sdl": sdl,
        "wgpu": wgpu,
        "dlss": dlss,
        "fsr": fsr,
        "note": "NVIDIA's NGX SDK and AMD's FidelityFX headers feed the Vulkan back end; host/mingw_msvc_shims.c is the MSVC runtime shim their static libraries need.",
    })
}

fn copy_if_size_differs(src: &Path, dst: &Path) -> Result<bool, String> {
    if !src.is_file() {
        return Ok(false);
    }
    let size = fs::metadata(src).map(|meta| meta.len()).unwrap_or(0);
    if dst.is_file() && fs::metadata(dst).map(|meta| meta.len()).unwrap_or(u64::MAX) == size {
        return Ok(false);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::copy(src, dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(true)
}

/// SDL (sdl2-compat loads SDL3.dll), wgpu-native and a shared FFmpeg's DLLs
/// beside the executable, as nx2/build.py copies them.
fn stage_windows_libraries(bin: &Path, options: &WindowsOptions) -> Result<Vec<String>, String> {
    let mut copied = Vec::new();
    if let Some(sdl) = &options.sdl {
        for name in ["SDL2.dll", "SDL3.dll"] {
            if copy_if_size_differs(&sdl.join(name), &bin.join(name))? {
                copied.push(name.to_owned());
            }
        }
    }
    if let Some(wgpu) = &options.wgpu {
        if copy_if_size_differs(
            &wgpu.join("lib/wgpu_native.dll"),
            &bin.join("wgpu_native.dll"),
        )? {
            copied.push("wgpu_native.dll".into());
        }
    }
    if let Some(ffmpeg) = &options.ffmpeg
        && let Ok(entries) = fs::read_dir(ffmpeg.join("bin"))
    {
        let mut names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if FFMPEG_LIBRARIES
                .iter()
                .any(|library| name.starts_with(&format!("{library}-")) && name.ends_with(".dll"))
                && copy_if_size_differs(&ffmpeg.join("bin").join(&name), &bin.join(&name))?
            {
                copied.push(name);
            }
        }
    }
    Ok(copied)
}

fn link_windows_json(exefs: &str, work: &str, out: &str) -> Result<Value, HostBuildError> {
    let root = crate::kit::root();
    let options = windows_options(&root, Path::new(work));
    link_windows_json_with(exefs, work, out, &options)
}

fn link_windows_json_with(
    exefs: &str,
    work: &str,
    out: &str,
    options: &WindowsOptions,
) -> Result<Value, HostBuildError> {
    let Some(toolchain) = options.toolchain.clone() else {
        let detail = options
            .toolchain_error
            .clone()
            .unwrap_or_else(|| "no llvm-mingw was found".into());
        return Err(HostBuildError::NoWindowsToolchain(detail));
    };
    link_windows_inner(exefs, work, out, options, &toolchain).map_err(HostBuildError::Build)
}

fn link_windows_inner(
    _exefs: &str,
    work: &str,
    out: &str,
    options: &WindowsOptions,
    toolchain: &MingwToolchain,
) -> Result<Value, String> {
    let root = crate::kit::root();
    let work = Path::new(work);
    let out = Path::new(out);
    if work.as_os_str().is_empty() || out.as_os_str().is_empty() {
        return Err("the Windows host link needs a work folder and an output folder".into());
    }
    let host = root.join("host");
    let opus = root.join("third_party/opus");
    let zstd = root.join("third_party/zstd/lib");
    let rt64 = work.join("rt64");
    let obj_dir = work.join("build/obj");
    let cache = work.join("build/objcache");
    fs::create_dir_all(&obj_dir).map_err(|e| format!("{}: {e}", obj_dir.display()))?;
    fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let cc = toolchain.clang.to_string_lossy().into_owned();
    let base = mingw_base_flags();
    let host_flags = windows_host_flags(
        &root,
        work,
        options.ffmpeg.as_deref(),
        options.dav1d.as_deref(),
    );
    let deps = windows_host_deps(
        &root,
        work,
        options.ffmpeg.as_deref(),
        options.dav1d.as_deref(),
    )?;
    let deps_file = work.join("build/host_deps.bin");
    write_blob(&deps_file, &deps)?;
    let mut prompts_deps = deps.clone();
    prompts_deps.extend(zstd_api_headers(&root));
    let prompts_deps_file = work.join("build/host_prompts_deps.bin");
    write_blob(&prompts_deps_file, &prompts_deps)?;
    let mut jobs = Vec::new();
    for src in windows_host_sources(
        &root,
        work,
        options.wgpu.is_some(),
        options.pgo_gen,
        options.dav1d.is_some(),
    )? {
        let stem = src.file_stem().and_then(|name| name.to_str()).unwrap_or("");
        let mut flags = host_flags.clone();
        flags.push(format!("-I{}", opus.join("include").display()));
        let generated = !src.starts_with(&host);
        let mut deps_file = &deps_file;
        if stem == "prompts" {
            flags.push(format!("-I{}", zstd.display()));
            deps_file = &prompts_deps_file;
        }
        if stem == "hos_main" {
            flags.push(crate::version::define(&root));
        }
        if stem == "hos_update" {
            flags.push(format!(
                "-DHOOKSHOT_REPOSITORY=\"{}\"",
                crate::version::repository(&root)
            ));
        }
        let obj = obj_dir.join(format!(
            "{}{}.o",
            if generated { "gen_" } else { "host_" },
            stem
        ));
        jobs.push(job(&cc, &src, &obj, &flags, deps_file, &cache));
    }
    jobs.extend(opus_jobs(&cc, &root, &obj_dir, &deps_file, &cache, &base)?);
    jobs.extend(zstd_jobs(&cc, &root, &obj_dir, &deps_file, &cache, &base));
    if let Some(wgpu) = &options.wgpu {
        let cxx = toolchain.clangxx.as_ref().ok_or_else(|| {
            format!(
                "{} has no clang++: the WebGPU renderer needs it, or point WGPU_NATIVE at nothing to build without that renderer",
                toolchain.clang.display()
            )
        })?;
        let gpu = root.join("gpu");
        let mut flags = mingw_target_flags();
        flags.extend([
            "-std=c++20".into(),
            "-O2".into(),
            "-g0".into(),
            "-DNDEBUG".into(),
            "-Wno-everything".into(),
            format!("-I{}", gpu.join("compat").display()),
            format!("-I{}", gpu.join("third_party/suyu").display()),
            format!("-I{}", wgpu.join("include").display()),
            format!("-I{}", host.display()),
            format!("-I{}", rt64.display()),
            "-DA64_WRITE_TRACK".into(),
            "-include".into(),
            gpu.join("render/rx_wgpu_names.h")
                .to_string_lossy()
                .into_owned(),
        ]);
        let mut gpu_deps = tree_headers(&[gpu.clone(), host.clone()], &["vk"]);
        gpu_deps.extend(fs::read(wgpu.join("include/webgpu/webgpu.h")).unwrap_or_default());
        let gpu_deps_file = work.join("build/gpu_deps.bin");
        write_blob(&gpu_deps_file, &gpu_deps)?;
        let cxx = cxx.to_string_lossy().into_owned();
        for src in gpu_sources(&gpu)? {
            let relative = src.strip_prefix(&gpu).unwrap_or(&src);
            let name = relative
                .to_string_lossy()
                .replace(['\\', '/'], "_")
                .trim_end_matches(".cpp")
                .to_owned();
            let obj = obj_dir.join(format!("gpu_{name}.o"));
            jobs.push(job(&cxx, &src, &obj, &flags, &gpu_deps_file, &cache));
        }
    }
    let parallel = crate::toolchain::default_jobs(3.5);
    let job_objects: Vec<String> = jobs
        .iter()
        .filter_map(|job| job["obj"].as_str().map(str::to_owned))
        .collect();
    let _ = crate::compile::write_compdb(&work.join("build").join(crate::compile::COMPDB), &jobs);
    let compiled = crate::compile::many_json(
        &json!({"jobs": jobs, "cache": cache, "parallel": parallel}),
        &root,
    )?;
    if compiled["failed"]
        .as_array()
        .is_some_and(|failed| !failed.is_empty())
    {
        return Ok(json!({
            "stage": "host-compile",
            "platform": "windows",
            "compiled": compiled,
            "inputs": windows_inputs(options),
        }));
    }
    let objects = collect_objects(work, &job_objects)?;
    let bin = out.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let exe = bin.join(Platform::Windows.executable());
    let map = exe.with_extension("map");
    let manifest = bin.join("hookshot_manifest.res");
    crate::win_manifest::write(&manifest)?;
    let resource = work.join("hookshot.res");
    let resource = resource.is_file().then_some(resource);
    let args = windows_link_prefix(&exe, &map);
    let after = windows_link_suffix(&manifest, resource.as_deref(), options);
    let link = crate::linker::link_json(&json!({
        "cc": cc,
        "out": exe.to_string_lossy(),
        "objects": objects,
        "args": args,
        "after": after,
    }))?;
    let size = fs::metadata(&exe).map(|meta| meta.len()).unwrap_or(0);
    let mut staged = Vec::new();
    if link["ok"].as_bool().unwrap_or(false) {
        staged = stage_windows_libraries(&bin, options)?;
    }
    Ok(json!({
        "stage": if link["ok"].as_bool().unwrap_or(false) { "host-linked" } else { "host-link" },
        "platform": "windows",
        "compiled": compiled,
        "link": link,
        "exe": exe.to_string_lossy(),
        "map": map.to_string_lossy(),
        "manifest": manifest.to_string_lossy(),
        "size": size,
        "ffmpeg": options.ffmpeg.is_some(),
        "wgpu": options.wgpu.is_some(),
        "sdl": options.sdl.as_ref().is_some_and(|folder| folder.is_dir()),
        "staged": staged,
        "inputs": windows_inputs(options),
        "toolchain": toolchain.clang.to_string_lossy(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-host-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn windows_source_list_matches_build_py() {
        let root = crate::kit::root();
        let work = temp("sources");
        fs::create_dir_all(work.join("sa/gen")).unwrap();
        for name in ["a64_stubs.c", "hos_image.c", "nvn2_exports.c"] {
            fs::write(work.join("sa/gen").join(name), "").unwrap();
        }
        let sources = windows_host_sources(&root, &work, false, false, false).unwrap();
        let names: Vec<String> = sources
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for wanted in [
            "display.c",
            "mf_av1.c",
            "pad_sdl.c",
            "mingw_msvc_shims.c",
            "msvc_oldnames.c",
            "rx_nowgpu.c",
            "hos_main.c",
            "hos_update.c",
            "a64_stubs.c",
            "hos_image.c",
            "nvn2_exports.c",
        ] {
            assert!(names.iter().any(|name| name == wanted), "{wanted} missing");
        }
        assert!(!names.iter().any(|name| name == "pgo_rt.c"));
        let expected = fs::read_dir(root.join("host"))
            .unwrap()
            .flatten()
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.ends_with(".c")
                    && name != "pgo_rt.c"
                    && name != "rx_nowgpu.c"
                    && name != "dav1d_av1.c"
            })
            .count();
        assert_eq!(sources.len(), expected + 4);

        let with_wgpu = windows_host_sources(&root, &work, true, false, false).unwrap();
        assert!(!with_wgpu.iter().any(|path| path.ends_with("rx_nowgpu.c")));
        let with_pgo = windows_host_sources(&root, &work, false, true, false).unwrap();
        assert!(with_pgo.iter().any(|path| path.ends_with("pgo_rt.c")));
        let with_dav1d = windows_host_sources(&root, &work, false, false, true).unwrap();
        assert!(
            with_dav1d.iter().any(|path| path.ends_with("dav1d_av1.c")),
            "dav1d_av1.c is compiled only with a dav1d in the CPM cache"
        );
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn windows_host_flags_match_build_py() {
        let root = crate::kit::root();
        let work = Path::new("/scratch/work");
        let ffmpeg = Path::new("/cache/ffmpeg/win64");
        let dav1d = Path::new("/cache/dav1d/1.5.4");
        let flags = windows_host_flags(&root, work, Some(ffmpeg), Some(dav1d));
        for wanted in [
            "--target=x86_64-w64-mingw32",
            "-fms-extensions",
            "-D_CRT_SECURE_NO_WARNINGS",
            "-ffp-contract=off",
            "-mcx16",
            "-O2",
            "-g0",
            "-Wno-everything",
            "-DA64_POLLING",
            "-DA64_WRITE_TRACK",
        ] {
            assert!(flags.iter().any(|flag| flag == wanted), "{wanted} missing");
        }
        assert!(
            flags.iter().any(|flag| flag.starts_with("-march=")),
            "the CPU level is missing"
        );
        assert!(flags.contains(&format!("-I{}", root.join("host").display())));
        assert!(flags.contains(&format!("-I{}", work.join("rt64").display())));
        assert!(flags.contains(&format!("-I{}", work.join("sa/gen").display())));
        assert!(flags.contains(&format!("-I{}", ffmpeg.join("include").display())));
        assert!(flags.iter().any(|flag| flag == "-DHOOKSHOT_DAV1D"));
        assert!(flags.contains(&format!("-I{}", dav1d.join("include").display())));
        assert!(!flags.iter().any(|flag| flag == "-D_GNU_SOURCE"));
        assert!(!flags.iter().any(|flag| flag.contains("host/posix")));
    }

    fn link_options(base: &Path) -> WindowsOptions {
        let ffmpeg = base.join("ffmpeg/win64");
        fs::create_dir_all(ffmpeg.join("lib")).unwrap();
        for library in FFMPEG_LIBRARIES {
            fs::write(ffmpeg.join("lib").join(format!("{library}.lib")), b"lib").unwrap();
        }
        let dav1d = base.join("dav1d/1.5.4");
        fs::create_dir_all(dav1d.join("lib")).unwrap();
        fs::write(dav1d.join("lib/libdav1d.a"), b"!<arch>\n").unwrap();
        fs::create_dir_all(dav1d.join("include/dav1d")).unwrap();
        fs::write(dav1d.join("include/dav1d/dav1d.h"), b"header").unwrap();
        let wgpu = base.join("wgpu-native");
        fs::create_dir_all(wgpu.join("lib")).unwrap();
        fs::write(wgpu.join("lib/wgpu_native.dll.lib"), b"lib").unwrap();
        let ngx = base.join("ngx/lib/nvsdk_ngx_s.lib");
        fs::create_dir_all(ngx.parent().unwrap()).unwrap();
        fs::write(&ngx, b"ngx").unwrap();
        let fsr = base.join("third_party/fidelityfx");
        fs::create_dir_all(&fsr).unwrap();
        let hot = base.join("work/hot_order.txt");
        fs::create_dir_all(hot.parent().unwrap()).unwrap();
        fs::write(&hot, "f_001\n").unwrap();
        WindowsOptions {
            ffmpeg: Some(ffmpeg),
            dav1d: Some(dav1d),
            wgpu: Some(wgpu),
            sdl: Some(base.join("third_party/sdl2")),
            ngx: Some(ngx),
            fsr: Some(fsr),
            hot_order: Some(hot),
            release: true,
            pgo_gen: true,
            ..WindowsOptions::default()
        }
    }

    #[test]
    fn windows_link_line_matches_build_py() {
        let base = temp("link");
        let options = link_options(&base);
        let out = base.join("out/é 游戏");
        let exe = out.join("bin/hookshot.exe");
        let map = exe.with_extension("map");
        let manifest = out.join("bin/hookshot_manifest.res");
        let resource = base.join("work/hookshot.res");
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::write(&resource, b"res").unwrap();
        let prefix = windows_link_prefix(&exe, &map);
        let suffix = windows_link_suffix(&manifest, Some(resource.as_path()), &options);
        for wanted in [
            "--target=x86_64-w64-mingw32",
            "-fms-extensions",
            "-D_CRT_SECURE_NO_WARNINGS",
            "-fuse-ld=lld",
            "-static",
            "-Wl,-Xlink=/nodefaultlib:libcmt",
            "-Wl,-Xlink=/nodefaultlib:oldnames",
            "-Wl,-Xlink=/nodefaultlib:libvcruntime",
            "-Wl,-Xlink=/nodefaultlib:msvcrt",
            "-Xlink=/stack:8388608",
        ] {
            assert!(prefix.iter().any(|arg| arg == wanted), "{wanted} missing");
        }
        assert!(prefix.iter().any(|arg| arg == "-o"));
        assert!(prefix.contains(&exe.to_string_lossy().into_owned()));
        assert!(
            prefix
                .iter()
                .any(|arg| arg == &format!("/map:{}", map.display())
                    || arg == &format!("-Xlink=/map:{}", map.display()))
        );
        for wanted in [
            "-lwinhttp",
            "-luser32",
            "-lwinmm",
            "-ladvapi32",
            "-lole32",
            "-lshell32",
            "-lgdi32",
            "-ldbghelp",
            "-lc++",
            "-lunwind",
            "-lbcrypt",
            "-lmfplat",
            "-lmfuuid",
            "-lstrmiids",
        ] {
            assert!(suffix.iter().any(|arg| arg == wanted), "{wanted} missing");
        }
        assert!(suffix.iter().any(|arg| arg.contains("/order:@")));
        assert!(suffix.iter().any(|arg| arg.contains("/ignore:4037")));
        assert!(suffix.iter().any(|arg| arg.contains("/subsystem:windows")));
        assert!(
            suffix
                .iter()
                .any(|arg| arg.contains("/entry:mainCRTStartup"))
        );
        assert!(
            suffix
                .iter()
                .any(|arg| arg.contains("/nodefaultlib:clang_rt.profile.lib"))
        );
        assert!(suffix.iter().any(|arg| arg.ends_with("avcodec.lib")));
        assert!(suffix.iter().any(|arg| arg.ends_with("libdav1d.a")));
        assert!(
            suffix
                .iter()
                .any(|arg| arg.ends_with("wgpu_native.dll.lib"))
        );
        assert!(suffix.contains(&resource.to_string_lossy().into_owned()));
        assert!(suffix.contains(&manifest.to_string_lossy().into_owned()));
        assert!(
            suffix.iter().any(|arg| arg.contains("é 游戏")),
            "UTF-8 paths are kept"
        );
        let inputs = windows_inputs(&options);
        assert!(inputs["ffmpeg"]["dir"].as_str().unwrap().contains("ffmpeg"));
        assert!(inputs["dav1d"].as_str().unwrap().contains("dav1d"));
        assert!(
            inputs["dlss"]["library"]
                .as_str()
                .unwrap()
                .contains("nvsdk_ngx_s.lib")
        );
        assert!(
            inputs["fsr"]["headers"]
                .as_str()
                .unwrap()
                .contains("fidelityfx")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_build_files_folder_with_a_comma_keeps_its_map_and_order_paths_whole() {
        let base = temp("link-comma");
        let mut options = link_options(&base);
        let out = base.join("Games, Zelda");
        let hot = out.join("hot_order.txt");
        fs::create_dir_all(&out).unwrap();
        fs::write(&hot, "f_001\n").unwrap();
        options.hot_order = Some(hot.clone());
        let exe = out.join("bin/hookshot.exe");
        let map = exe.with_extension("map");
        let prefix = windows_link_prefix(&exe, &map);
        // (each link option that names a file goes as one argument after -Xlinker: -Wl, would split it at the comma)
        let map_pair = prefix
            .windows(2)
            .find(|pair| pair[1].starts_with("-Xlink=/map:"))
            .expect("the game's link map");
        assert_eq!(map_pair[0], "-Xlinker");
        assert_eq!(map_pair[1], format!("-Xlink=/map:{}", map.display()));
        let suffix = windows_link_suffix(&out.join("hookshot_manifest.res"), None, &options);
        let order_pair = suffix
            .windows(2)
            .find(|pair| pair[1].starts_with("-Xlink=/order:@"))
            .expect("the hot order file");
        assert_eq!(order_pair[0], "-Xlinker");
        assert_eq!(
            order_pair[1],
            format!(
                "-Xlink=/order:@{}",
                hot.to_string_lossy().replace('\\', "/")
            )
        );
        for argument in prefix.iter().chain(suffix.iter()) {
            if argument.starts_with("-Wl,") {
                assert!(
                    !argument.contains("Games, Zelda"),
                    "a path through -Wl, would split at the comma: {argument}"
                );
            }
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn ffmpeg_comes_from_a_cpm_cache_and_skips_linux_builds() {
        let base = temp("ffmpeg");
        let win = base.join("cpm/ffmpeg/win64");
        let linux = base.join("cpm/ffmpeg/linux-x64");
        fs::create_dir_all(win.join("lib")).unwrap();
        fs::create_dir_all(linux.join("lib")).unwrap();
        assert_eq!(find_ffmpeg_in(vec![base.join("cpm")]), Some(win));
        fs::remove_dir_all(base.join("cpm/ffmpeg/win64")).unwrap();
        assert_eq!(find_ffmpeg_in(vec![base.join("cpm")]), None);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn dav1d_comes_from_a_cpm_cache_with_its_header_and_library() {
        let base = temp("dav1d");
        let package = base.join("cpm/dav1d/1.5.4");
        fs::create_dir_all(package.join("lib")).unwrap();
        fs::create_dir_all(package.join("include/dav1d")).unwrap();
        fs::write(package.join("lib/libdav1d.a"), b"!<arch>\n").unwrap();
        fs::write(package.join("include/dav1d/dav1d.h"), b"header").unwrap();
        assert_eq!(find_dav1d_in(vec![base.join("cpm")]), Some(package.clone()));
        fs::remove_file(package.join("lib/libdav1d.a")).unwrap();
        assert_eq!(find_dav1d_in(vec![base.join("cpm")]), None);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_libraries_are_staged_beside_the_executable() {
        let base = temp("stage");
        let options = link_options(&base);
        let ffmpeg = options.ffmpeg.as_ref().unwrap();
        fs::create_dir_all(ffmpeg.join("bin")).unwrap();
        fs::write(ffmpeg.join("bin/avcodec-60.dll"), b"dll").unwrap();
        fs::write(ffmpeg.join("bin/avutil-58.dll"), b"dll").unwrap();
        let sdl = options.sdl.as_ref().unwrap();
        fs::create_dir_all(sdl).unwrap();
        fs::write(sdl.join("SDL2.dll"), b"SDL2").unwrap();
        fs::write(sdl.join("SDL3.dll"), b"SDL3").unwrap();
        fs::write(
            options.wgpu.as_ref().unwrap().join("lib/wgpu_native.dll"),
            b"wgpu",
        )
        .unwrap();
        let bin = base.join("out/bin");
        let staged = stage_windows_libraries(&bin, &options).unwrap();
        for name in [
            "SDL2.dll",
            "SDL3.dll",
            "wgpu_native.dll",
            "avcodec-60.dll",
            "avutil-58.dll",
        ] {
            assert!(bin.join(name).is_file(), "{name} was not staged");
            assert!(staged.iter().any(|staged| staged == name), "{name}");
        }
        // (a second pass of the same size copies nothing)
        assert!(stage_windows_libraries(&bin, &options).unwrap().is_empty());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_missing_windows_toolchain_is_a_typed_error() {
        let error = link_windows_json_with(
            "",
            "",
            "",
            &WindowsOptions {
                toolchain_error: Some("no llvm-mingw was found: test".into()),
                ..WindowsOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, HostBuildError::NoWindowsToolchain(_)));
        let message = error.to_string();
        assert!(message.contains("llvm-mingw"), "{message}");
        assert!(
            message.contains("HOOKSHOT_MINGW") || message.contains("test"),
            "{message}"
        );
    }

    #[test]
    fn the_no_toolchain_message_names_llvm_mingw() {
        let Err(message) = find_windows_toolchain() else {
            return; // a toolchain is installed here: nothing to report
        };
        assert!(message.contains("llvm-mingw"), "{message}");
        assert!(message.contains("HOOKSHOT_MINGW"), "{message}");
        let error = link_windows_json_with(
            "",
            "",
            "",
            &WindowsOptions {
                toolchain_error: Some(message),
                ..WindowsOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, HostBuildError::NoWindowsToolchain(_)));
        assert!(error.to_string().contains("llvm-mingw"));
    }

    #[test]
    fn an_llvm_mingw_folder_is_found() {
        let base = temp("mingw");
        let bin = base.join("bin");
        fs::create_dir_all(&bin).unwrap();
        for name in ["clang", "clang++"] {
            fs::write(bin.join(name), b"").unwrap();
        }
        let toolchain = toolchain_in_folder(&base).unwrap();
        assert_eq!(toolchain.clang, bin.join("clang"));
        assert_eq!(toolchain.clangxx, Some(bin.join("clang++")));
        assert!(toolchain_in_folder(&base.join("missing")).is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn linux_host_link_still_checks_its_inputs() {
        let error = link_json_on(Platform::Linux, "", "", "").unwrap_err();
        assert!(matches!(error, HostBuildError::Build(_)));
        assert!(error.to_string().contains("has no main module"));
    }
}
