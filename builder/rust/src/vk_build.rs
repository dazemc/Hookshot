//! The Vulkan back end: `gpu/vk` and the shader recompiler it carries,
//! compiled into `work/sa/build/hookshot_vk.so` (Linux) or
//! `work/sa/build/hookshot_vk.dll` (Windows with llvm-mingw). Ported from
//! nx2/build.py's `vk_sources`, `vflags` and `vk_jobs` blocks.
//!
//! NVIDIA's DLSS SDK (`HOOKSHOT_NGX_SDK` or the request's `ngx` folder) adds
//! `-DHOOKSHOT_NGX` and its headers to `vk_dlss.cpp` and links its static
//! library; on Windows it also compiles `gpu/vk/vk_ngx_msvc_rt.cpp` and
//! `host/mingw_msvc_shims.c` for what NVIDIA's MSVC-built library asks of
//! Microsoft's runtime. AMD's vendored FSR 4 headers do the same for
//! `vk_fsr4.cpp` when `third_party/fidelityfx` is there. Without either,
//! those files compile their "not available" stubs, as build.py's do.
//!
//! Header roots come from `HOOKSHOT_VULKAN_HEADERS`, `HOOKSHOT_SPIRV_HEADERS`
//! and `HOOKSHOT_VMA_HEADERS` first, then the system's `/usr` and
//! `/usr/local`, then a CPM cache (`RX_CPM`, `cpm/` in the clone, or the old
//! project caches). Each names a package root with `include/`, or its
//! `include/` folder itself.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::platform::Platform;

/// NVIDIA's NGX runtime shim: build.py compiles it only for the Windows DLL
/// with the NGX SDK, never for the Linux shared object.
const NGX_MSVC_RT: &str = "vk_ngx_msvc_rt.cpp";

/// The two shims a Windows NGX DLL adds: NVIDIA's C++ runtime parts and the
/// C ones Microsoft's static libraries ask of `/MT` (`host/mingw_msvc_shims.c`,
/// shared with the static FFmpeg libraries). The object names are build.py's.
pub fn windows_ngx_sources(root: &Path) -> [(&'static str, PathBuf); 2] {
    [
        ("vk_ngx_msvc_rt.o", root.join("gpu/vk").join(NGX_MSVC_RT)),
        (
            "vk_ngx_mingw_msvc_shims.o",
            root.join("host/mingw_msvc_shims.c"),
        ),
    ]
}

/// zstd's decoder, plain C, as build.py's `zstd_jobs` lists it.
const ZSTD_DECODER: [&str; 10] = [
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
];

/// Dear ImGui's objects: the overlays' library and its Vulkan backend.
const IMGUI_SOURCES: [&str; 5] = [
    "imgui.cpp",
    "imgui_draw.cpp",
    "imgui_tables.cpp",
    "imgui_widgets.cpp",
    "backends/imgui_impl_vulkan.cpp",
];

/// The two back-end sources that also see Dear ImGui's headers.
const IMGUI_USERS: [&str; 2] = ["vk_imgui.cpp", "vk_menu.cpp"];

/// The header roots the compile needs, each with an `include/` folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    pub vulkan: PathBuf,
    pub spirv: PathBuf,
    pub vma: PathBuf,
}

impl Roots {
    pub fn json(&self) -> Value {
        json!({
            "vulkan": self.vulkan.to_string_lossy(),
            "spirv": self.spirv.to_string_lossy(),
            "vma": self.vma.to_string_lossy(),
        })
    }
}

/// What the environment names for each header root.
#[derive(Debug, Clone, Default)]
pub struct HeaderEnv {
    pub vulkan: Option<PathBuf>,
    pub spirv: Option<PathBuf>,
    pub vma: Option<PathBuf>,
}

impl HeaderEnv {
    pub fn from_environment() -> Self {
        let read = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Self {
            vulkan: read("HOOKSHOT_VULKAN_HEADERS"),
            spirv: read("HOOKSHOT_SPIRV_HEADERS"),
            vma: read("HOOKSHOT_VMA_HEADERS"),
        }
    }
}

/// Where a header root is looked for: the environment's overrides, the
/// system's prefixes, then CPM package caches.
#[derive(Debug, Clone, Default)]
pub struct HeaderSearch {
    pub system: Vec<PathBuf>,
    pub cpm: Vec<PathBuf>,
}

impl HeaderSearch {
    /// The roots build.py can find in this checkout: the distro's prefixes,
    /// then a CPM cache. `RX_CPM` first, then `cpm/` in the clone and the
    /// older project caches.
    pub fn for_repo(root: &Path, work: &Path) -> Self {
        let mut cpm = Vec::new();
        let mut add = |path: PathBuf| {
            if path.is_dir() && !cpm.contains(&path) {
                cpm.push(path);
            }
        };
        if let Some(dir) = std::env::var_os("RX_CPM").filter(|dir| !dir.is_empty()) {
            add(PathBuf::from(dir));
        }
        add(root.join("cpm"));
        add(work.join("eg/.cache/cpm"));
        if let Some(parent) = root.parent() {
            add(parent.join("mkw-recomp/work/eg/.cache/cpm"));
            add(parent.join("Eglantine/.cache/cpm"));
        }
        Self {
            system: vec![PathBuf::from("/usr"), PathBuf::from("/usr/local")],
            cpm,
        }
    }
}

/// Whether a package root has `include/<probe>`.
fn has_include(root: &Path, probe: &str) -> bool {
    root.join("include").join(probe).is_file()
}

/// The newest version folder of a CPM package, when it holds a probe.
fn cpm_package(cpm: &[PathBuf], name: &str, probe: &str) -> Option<PathBuf> {
    for cache in cpm {
        let folder = cache.join(name);
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.path())
            .collect();
        versions.sort();
        if let Some(version) = versions.pop()
            && has_include(&version, probe)
        {
            return Some(version);
        }
    }
    None
}

/// One package root with `include/<probe>`: the environment's path (package
/// root or its `include/` folder), then the system's prefixes, then CPM.
pub fn header_package(
    search: &HeaderSearch,
    cpm_name: &str,
    probe: &str,
    environment: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(given) = environment.filter(|path| !path.as_os_str().is_empty()) {
        let given = if given.is_absolute() {
            given.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|dir| dir.join(given))
                .unwrap_or_else(|_| given.to_path_buf())
        };
        if has_include(&given, probe) {
            return Some(given);
        }
        // (`HOOKSHOT_*_HEADERS=<root>/include` is accepted too)
        if given.join(probe).is_file()
            && given.file_name().is_some_and(|name| name == "include")
            && let Some(parent) = given.parent()
        {
            return Some(parent.to_path_buf());
        }
    }
    for prefix in &search.system {
        if has_include(prefix, probe) {
            return Some(prefix.clone());
        }
    }
    cpm_package(&search.cpm, cpm_name, probe)
}

/// The three header packages, each when it was found.
#[derive(Debug, Clone, Default)]
pub struct Headers {
    pub vulkan: Option<PathBuf>,
    pub spirv: Option<PathBuf>,
    pub vma: Option<PathBuf>,
}

impl Headers {
    pub fn find(search: &HeaderSearch, environment: &HeaderEnv) -> Self {
        Self {
            vulkan: header_package(
                search,
                "vulkanheaders",
                "vulkan/vulkan.h",
                environment.vulkan.as_deref(),
            ),
            spirv: header_package(
                search,
                "spirv-headers",
                "spirv/unified1/spirv.h",
                environment.spirv.as_deref(),
            ),
            vma: header_package(
                search,
                "vulkanmemoryallocator",
                "vk_mem_alloc.h",
                environment.vma.as_deref(),
            ),
        }
    }

    /// The labels build.py names in its message, in its order.
    pub fn missing(&self) -> Vec<&'static str> {
        [
            ("Vulkan", &self.vulkan),
            ("SPIR-V", &self.spirv),
            ("VMA", &self.vma),
        ]
        .into_iter()
        .filter(|(_, root)| root.is_none())
        .map(|(name, _)| name)
        .collect()
    }

    pub fn roots(&self) -> Option<Roots> {
        Some(Roots {
            vulkan: self.vulkan.clone()?,
            spirv: self.spirv.clone()?,
            vma: self.vma.clone()?,
        })
    }
}

/// build.py's sentence when a header package is missing, unchanged but for
/// the library's name.
#[cfg(test)]
#[allow(dead_code)] // (the headers path reports the same sentences through its error)
pub fn missing_guidance(missing: &[&str]) -> String {
    missing_guidance_for(Platform::current(), missing)
}

pub fn missing_guidance_for(platform: Platform, missing: &[&str]) -> String {
    format!(
        "no headers for {} (missing {}). Set RX_CPM to a CPM cache, or HOOKSHOT_VULKAN_HEADERS / \
         HOOKSHOT_SPIRV_HEADERS / HOOKSHOT_VMA_HEADERS (package root or its include/ folder), or install \
         distro packages vulkan-headers + spirv-headers + vulkan-memory-allocator, or set HOOKSHOT_VK_PREBUILT.",
        platform.vulkan_library(),
        missing.join(", ")
    )
}

/// What this build asks of the optional SDKs beyond the headers.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// NVIDIA's DLSS SDK folder: the request's `ngx` path, else
    /// `HOOKSHOT_NGX_SDK` (what build.py reads).
    pub ngx: Option<PathBuf>,
}

/// Why the Vulkan back end stopped before it could report a compile or link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VkBuildError {
    /// The Windows DLL needs llvm-mingw, and none that can link for
    /// x86_64-w64-mingw32 was found.
    NoWindowsToolchain(String),
    /// The build could not run, or the compiler/linker rejected it.
    Build(String),
}

impl std::fmt::Display for VkBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindowsToolchain(message) | Self::Build(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for VkBuildError {}

impl From<String> for VkBuildError {
    fn from(message: String) -> Self {
        Self::Build(message)
    }
}

impl From<VkBuildError> for String {
    fn from(error: VkBuildError) -> Self {
        error.to_string()
    }
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// The DLSS SDK folder a build reads: the request's, else `HOOKSHOT_NGX_SDK`.
/// No check of what it holds: the build fetches what is missing first.
pub fn ngx_folder(requested: Option<&Path>) -> Option<PathBuf> {
    requested
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::var_os("HOOKSHOT_NGX_SDK")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .map(|path| absolute(&path))
}

/// The DLSS SDK folder a build reads, checked: one that lacks this
/// platform's static library is an error rather than a silent build without
/// DLSS, because the caller asked for it.
#[cfg(test)]
pub fn resolve_ngx(requested: Option<&Path>) -> Result<Option<PathBuf>, String> {
    resolve_ngx_for(requested, Platform::current())
}

fn ngx_platform(platform: Platform) -> crate::ngx_sdk::Platform {
    match platform {
        Platform::Linux => crate::ngx_sdk::Platform::Linux,
        Platform::Windows => crate::ngx_sdk::Platform::Windows,
    }
}

pub fn resolve_ngx_for(
    requested: Option<&Path>,
    platform: Platform,
) -> Result<Option<PathBuf>, String> {
    let Some(sdk) = ngx_folder(requested) else {
        return Ok(None);
    };
    let library = crate::ngx_sdk::library(&sdk, ngx_platform(platform));
    if !library.is_file() {
        return Err(format!(
            "NVIDIA DLSS: {} has no {} (build.ngx_fetch, or scripts/ngx_sdk.py, fetches it): \
             the Vulkan back end was not built. Unset HOOKSHOT_NGX_SDK to build without DLSS.",
            sdk.display(),
            library.display()
        ));
    }
    Ok(Some(sdk))
}

/// `-DHOOKSHOT_NGX` and NVIDIA's headers, for `vk_dlss.cpp` alone.
pub fn ngx_flags(sdk: &Path) -> Vec<String> {
    vec![
        "-DHOOKSHOT_NGX".to_owned(),
        format!("-I{}", sdk.join("include").display()),
    ]
}

/// AMD's FSR 4 API headers, vendored under `third_party/fidelityfx` (MIT).
pub fn fsr_headers(root: &Path) -> Option<PathBuf> {
    let folder = root.join("third_party/fidelityfx");
    folder
        .join("api/include/ffx_api.h")
        .is_file()
        .then_some(folder)
}

/// `-DHOOKSHOT_FSR` and AMD's header root, for `vk_fsr4.cpp` alone.
pub fn fsr_flags(folder: &Path) -> Vec<String> {
    vec![
        "-DHOOKSHOT_FSR".to_owned(),
        format!("-I{}", folder.display()),
    ]
}

/// The link's tail: NVIDIA's static library before the system libraries, as
/// build.py writes `@objs.rsp <libnvsdk_ngx.a> -ldl -lpthread -lm`.
#[cfg(test)]
pub fn link_libraries(ngx: Option<&Path>) -> Vec<String> {
    link_libraries_for(Platform::current(), ngx)
}

pub fn link_libraries_for(platform: Platform, ngx: Option<&Path>) -> Vec<String> {
    if platform == Platform::Windows {
        // (build.py's Windows link tail: the Win32 and C++ runtime libraries,
        // then NVIDIA's static library and the registry calls it makes. The
        // /MT-built library names Microsoft's libcpmt, which llvm-mingw does
        // not have: vk_ngx_msvc_rt.cpp supplies what it needs of it)
        let mut libraries = ["-luser32", "-lc++", "-lunwind"]
            .map(str::to_owned)
            .to_vec();
        if let Some(sdk) = ngx {
            libraries.push(
                crate::ngx_sdk::library(sdk, crate::ngx_sdk::Platform::Windows)
                    .to_string_lossy()
                    .into_owned(),
            );
            libraries.push("-ladvapi32".to_owned());
            libraries.push("-Wl,-Xlink=/nodefaultlib:libcpmt".to_owned());
        }
        return libraries;
    }
    let mut libraries = Vec::new();
    if let Some(sdk) = ngx {
        libraries.push(
            crate::ngx_sdk::library(sdk, crate::ngx_sdk::Platform::Linux)
                .to_string_lossy()
                .into_owned(),
        );
    }
    libraries.extend(["-ldl", "-lpthread", "-lm"].map(str::to_owned));
    libraries
}

fn cpp_files(folder: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = entry.path();
        if path.is_dir() {
            cpp_files(&path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("cpp") {
            out.push(path);
        }
    }
    Ok(())
}

/// build.py's `vk_sources`: `gpu/vk/*.cpp` without the NGX shim, Eden's and
/// sirit's trees, and suyu's texture decoders (the back end's unswizzle).
pub fn sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let vk = root.join("gpu/vk");
    let mut out = Vec::new();
    cpp_files(&vk.join("eden"), &mut out)?;
    cpp_files(&vk.join("sirit/src"), &mut out)?;
    if let Ok(entries) = fs::read_dir(&vk) {
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", vk.display()))?;
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some("cpp")
                && path.file_name().and_then(|name| name.to_str()) != Some(NGX_MSVC_RT)
            {
                out.push(path);
            }
        }
    }
    let textures = root.join("gpu/third_party/suyu/video_core/textures");
    out.push(textures.join("decoders.cpp"));
    out.push(textures.join("astc.cpp"));
    out.sort();
    Ok(out)
}

/// `-march` (RX_MARCH when named), `-ffp-contract=off` and a known tune, as
/// build.py's `CPU` list orders them.
fn cpu_flags() -> Vec<String> {
    let march = std::env::var("RX_MARCH")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(crate::host::march);
    cpu_flags_with(&march, crate::host::mtune_flag().as_deref())
}

fn cpu_flags_with(march: &str, tune: Option<&str>) -> Vec<String> {
    let mut flags = vec![format!("-march={march}"), "-ffp-contract=off".to_owned()];
    if let Some(tune) = tune {
        flags.push(format!("-mtune={tune}"));
    }
    flags
}

fn release_requested() -> bool {
    std::env::var("RX_RELEASE").as_deref() == Ok("1")
}

/// build.py's `vflags`, less the per-file defines: the forced includes, C++20,
/// NDEBUG and the back end's defines, then its include order. Eden's
/// recompiler comes before suyu's root (`-DMKWVK_INCLUDES=2`), which object
/// keys see because include folders themselves are left out of them.
#[cfg(test)]
pub fn vk_flags(root: &Path, gen_dir: &Path, headers: &Roots, release: bool) -> Vec<String> {
    vk_flags_for(Platform::Linux, root, gen_dir, headers, release)
}

/// build.py's `vflags` for either target: the Windows DLL uses llvm-mingw's
/// target flags and leaves out the POSIX shim, `-fPIC` and `_GNU_SOURCE`.
pub fn vk_flags_for(
    platform: Platform,
    root: &Path,
    gen_dir: &Path,
    headers: &Roots,
    release: bool,
) -> Vec<String> {
    let vk = root.join("gpu/vk");
    let mut flags = if platform == Platform::Windows {
        crate::host_build::mingw_target_flags()
    } else {
        crate::toolchain::native_target(false)
    };
    flags.extend(cpu_flags());
    if platform == Platform::Linux {
        flags.extend([
            "-fPIC".to_owned(),
            format!("-I{}", root.join("host/posix").display()),
            "-D_GNU_SOURCE".to_owned(),
        ]);
    }
    flags.extend([
        "-include".to_owned(),
        "algorithm".to_owned(),
        "-include".to_owned(),
        "cstdio".to_owned(),
        "-include".to_owned(),
        "cstdlib".to_owned(),
        "-include".to_owned(),
        "cstring".to_owned(),
        "-include".to_owned(),
        "functional".to_owned(),
        "-include".to_owned(),
        "string".to_owned(),
        "-include".to_owned(),
        "vector".to_owned(),
        "-std=c++20".to_owned(),
        "-O2".to_owned(),
        "-g0".to_owned(),
        "-DNDEBUG".to_owned(),
        "-Wno-everything".to_owned(),
        "-DVK_NO_PROTOTYPES".to_owned(),
        "-DMKWVK_BUILDING".to_owned(),
    ]);
    if release {
        flags.push("-DHOOKSHOT_RELEASE".to_owned());
    }
    flags.push("-DMKWVK_INCLUDES=2".to_owned());
    flags.extend([
        format!("-I{}", vk.join("compat").display()),
        format!("-I{}", root.join("gpu/compat").display()),
        format!("-I{}", vk.join("eden").display()),
        format!("-I{}", root.join("gpu/third_party/suyu").display()),
        format!("-I{}", vk.join("sirit/include").display()),
        format!("-I{}", vk.join("sirit/src").display()),
        format!("-I{}", headers.spirv.join("include").display()),
        format!(
            "-I{}",
            headers.spirv.join("include/spirv/unified1").display()
        ),
        format!("-I{}", headers.vulkan.join("include").display()),
        format!("-I{}", headers.vma.join("include").display()),
        format!("-I{}", vk.display()),
        format!("-I{}", gen_dir.display()),
    ]);
    flags
}

/// Dear ImGui's own defines (the overlays draw over the window).
pub fn imgui_flags(root: &Path) -> Vec<String> {
    vec![
        format!("-I{}", root.join("third_party/imgui").display()),
        format!("-I{}", root.join("third_party/zstd/lib").display()),
        "-DIMGUI_IMPL_VULKAN_NO_PROTOTYPES".to_owned(),
        "-DIMGUI_DISABLE_DEFAULT_SHELL_FUNCTIONS".to_owned(),
        "-DIMGUI_DISABLE_WIN32_FUNCTIONS".to_owned(),
        "-DIMGUI_DISABLE_DEMO_WINDOWS".to_owned(),
        "-DIMGUI_DISABLE_DEBUG_TOOLS".to_owned(),
    ]
}

/// zstd's flags in the back end: a hidden, static decoder.
pub fn zstd_flags_for(platform: Platform) -> Vec<String> {
    let mut flags = if platform == Platform::Windows {
        crate::host_build::mingw_target_flags()
    } else {
        crate::toolchain::native_target(false)
    };
    flags.extend(cpu_flags());
    if platform == Platform::Linux {
        flags.push("-fPIC".to_owned());
    }
    flags.extend([
        "-O2".to_owned(),
        "-g0".to_owned(),
        "-DNDEBUG".to_owned(),
        "-Wno-everything".to_owned(),
        "-fvisibility=hidden".to_owned(),
        "-DZSTD_DISABLE_ASM".to_owned(),
        "-DZSTD_LEGACY_SUPPORT=0".to_owned(),
        "-DZSTD_MULTITHREAD=0".to_owned(),
    ]);
    flags
}

/// The bundled fonts' object: no optimisation, no warnings.
pub fn ui_fonts_flags_for(platform: Platform) -> Vec<String> {
    let mut flags = if platform == Platform::Windows {
        crate::host_build::mingw_target_flags()
    } else {
        crate::toolchain::native_target(false)
    };
    if platform == Platform::Linux {
        flags.push("-fPIC".to_owned());
    }
    flags.extend(["-O0".to_owned(), "-g0".to_owned(), "-w".to_owned()]);
    flags
}

/// The Win32 calls on POSIX, compiled into the back end.
pub fn posix_flags(root: &Path) -> Vec<String> {
    let mut flags = crate::toolchain::native_target(false);
    flags.extend(cpu_flags());
    flags.extend([
        "-fPIC".to_owned(),
        "-O2".to_owned(),
        "-g0".to_owned(),
        "-Wno-everything".to_owned(),
        "-D_GNU_SOURCE".to_owned(),
        format!("-I{}", root.join("host/posix").display()),
    ]);
    flags
}

fn walk_files(folder: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn walk_headers(folder: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
        let path = entry.path();
        if path.is_dir() {
            walk_headers(&path, out)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext, "h" | "hpp" | "inc"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn digest_files<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) -> Result<String, String> {
    let mut found: Vec<&PathBuf> = paths.into_iter().collect();
    found.sort();
    let mut digest = Sha1::new();
    for path in found {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        digest.update(name.as_bytes());
        let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        digest.update(data);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// build.py's `tree_headers`: every header under these roots, by name and
/// content, as one fingerprint.
pub fn tree_headers(roots: &[PathBuf]) -> Result<String, String> {
    let mut found = Vec::new();
    for root in roots {
        walk_headers(root, &mut found)?;
    }
    digest_files(&found)
}

/// The headers directly in these folders (no walk), as Dear ImGui's
/// dependency blob in build.py.
fn flat_headers(folders: &[PathBuf]) -> Result<String, String> {
    let mut found = Vec::new();
    for folder in folders {
        let Ok(entries) = fs::read_dir(folder) else {
            continue;
        };
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", folder.display()))?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("h") {
                found.push(path);
            }
        }
    }
    digest_files(&found)
}

/// zstd's headers, concatenated as build.py's `zstd_jobs` reads them.
pub fn zstd_headers(zstd: &Path) -> Result<String, String> {
    let mut paths = Vec::new();
    for folder in [
        zstd.to_path_buf(),
        zstd.join("common"),
        zstd.join("decompress"),
    ] {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        let mut here: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("h")
            })
            .collect();
        here.sort();
        paths.extend(here);
    }
    let mut digest = Sha1::new();
    for path in paths {
        let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        digest.update(data);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// What makes the shader caches good for this translator (build.py's
/// `HOOKSHOT_SHADER_STAMP`): Eden's, sirit's and the compat headers' files,
/// then the four back-end files the translation goes through. Line endings
/// are normalised, so a checkout's CRLF does not change the stamp.
pub fn shader_stamp(root: &Path) -> Result<String, String> {
    let vk = root.join("gpu/vk");
    let mut files = Vec::new();
    for top in ["eden", "sirit", "compat"] {
        walk_files(&vk.join(top), &mut files)?;
    }
    for name in ["vk_shader.cpp", "vk_bank.cpp", "vk_state.h", "vk_api.h"] {
        files.push(vk.join(name));
    }
    files.sort();
    let mut digest = Sha1::new();
    for path in files {
        let relative = path
            .strip_prefix(&vk)
            .unwrap_or(&path)
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        digest.update(relative.as_bytes());
        let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut line_endings = 0;
        while line_endings < data.len() {
            if data[line_endings] == b'\r' && data.get(line_endings + 1) == Some(&b'\n') {
                line_endings += 1;
                continue;
            }
            digest.update([data[line_endings]]);
            line_endings += 1;
        }
    }
    Ok(format!("{:x}", digest.finalize())[..16].to_owned())
}

pub fn shader_stamp_define(stamp: &str) -> String {
    format!("-DHOOKSHOT_SHADER_STAMP=0x{stamp}ull")
}

/// The object name build.py gives a back-end source: its path under `gpu/vk`
/// with underscores, and `vk_` in front. Sources outside `gpu/vk` keep the
/// `..` Python's `os.path.relpath` writes, as underscores.
fn object_name(relative: &Path) -> String {
    let stem = relative
        .to_string_lossy()
        .trim_end_matches(".cpp")
        .replace(['/', '\\'], "_");
    format!("vk_{stem}.o")
}

/// `os.path.relpath` for the back end's object names: the `..` steps out of
/// `gpu/vk` are part of the name, as build.py makes them.
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let path_parts: Vec<_> = path.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let common = path_parts
        .iter()
        .zip(&base_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut out = PathBuf::new();
    for _ in common..base_parts.len() {
        out.push("..");
    }
    for part in &path_parts[common..] {
        out.push(part.as_os_str());
    }
    out
}

fn write_deps(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn job(cc: &str, src: &Path, obj: &Path, flags: &[String], deps: &Path, cache: &Path) -> Value {
    json!({
        "cc": cc,
        "src": src.to_string_lossy(),
        "obj": obj.to_string_lossy(),
        "cache": cache.to_string_lossy(),
        "flags": flags,
        "deps_file": if deps.as_os_str().is_empty() { String::new() } else { deps.to_string_lossy().into_owned() },
    })
}

/// The skipped answer: build.all keeps going and packages the game without a
/// renderer, and the builder's log says why rather than leaving a black
/// window.
#[cfg(test)]
fn skipped(missing: &[&str]) -> Value {
    skipped_for(Platform::current(), missing)
}

fn skipped_for(platform: Platform, missing: &[&str]) -> Value {
    json!({
        "stage": "vulkan",
        "built": false,
        "reason": "headers",
        "missing": missing,
        "message": missing_guidance_for(platform, missing),
    })
}

/// Compile and link `hookshot_vk.so` (Linux) or `hookshot_vk.dll` (Windows)
/// into `work/sa/build`.
pub fn build_json(work: &str) -> Result<Value, VkBuildError> {
    let work = if work.is_empty() {
        crate::kit::work()
    } else {
        PathBuf::from(work)
    };
    let root = crate::kit::root();
    build_at(&root, &work)
}

/// The same, from an explicit repository root and work folder: the tests and
/// the build driver use it. `HOOKSHOT_NGX_SDK` still names NVIDIA's SDK.
pub fn build_at(root: &Path, work: &Path) -> Result<Value, VkBuildError> {
    build_at_with(
        root,
        work,
        &HeaderSearch::for_repo(root, work),
        &HeaderEnv::from_environment(),
    )
}

/// Ported build.py path, with the header search callers can supply (a test
/// with no headers must not find this PC's).
pub fn build_at_with(
    root: &Path,
    work: &Path,
    search: &HeaderSearch,
    environment: &HeaderEnv,
) -> Result<Value, VkBuildError> {
    build_at_with_options_search(root, work, search, environment, &Options::default())
}

/// The same, with the optional SDKs (NVIDIA's DLSS, AMD's FSR 4 headers).
pub fn build_at_with_options(
    root: &Path,
    work: &Path,
    options: &Options,
) -> Result<Value, VkBuildError> {
    build_at_with_options_search(
        root,
        work,
        &HeaderSearch::for_repo(root, work),
        &HeaderEnv::from_environment(),
        options,
    )
}

pub fn build_at_with_options_search(
    root: &Path,
    work: &Path,
    search: &HeaderSearch,
    environment: &HeaderEnv,
    options: &Options,
) -> Result<Value, VkBuildError> {
    build_at_for(
        Platform::current(),
        root,
        work,
        search,
        environment,
        options,
        None,
    )
}

/// The same, for an explicit target. `toolchain` lets a test supply the
/// llvm-mingw answer instead of this PC's; `None` discovers it as the host
/// link does.
fn build_at_for(
    platform: Platform,
    root: &Path,
    work: &Path,
    search: &HeaderSearch,
    environment: &HeaderEnv,
    options: &Options,
    toolchain: Option<Result<crate::host_build::MingwToolchain, String>>,
) -> Result<Value, VkBuildError> {
    // (clang on Windows opens a \\?\C:\... source but does not resolve its
    // quoted includes beside it; the build uses the plain path)
    let root = &crate::kit::plain(root.to_path_buf());
    let work = &crate::kit::plain(work.to_path_buf());
    // (a Windows cross build must not put this PC's /usr/include in front of
    // llvm-mingw's headers: Linux's stdio.h and stdint.h do not belong in a
    // PE compile. Environment names and the CPM cache still supply them)
    let cross_search;
    let search = if platform == Platform::Windows && !search.system.is_empty() {
        cross_search = HeaderSearch {
            system: Vec::new(),
            cpm: search.cpm.clone(),
        };
        &cross_search
    } else {
        search
    };
    let headers = Headers::find(search, environment);
    let Some(roots) = headers.roots() else {
        return Ok(skipped_for(platform, &headers.missing()));
    };
    // (NVIDIA's DLSS: the request's SDK folder, else HOOKSHOT_NGX_SDK)
    let ngx = resolve_ngx_for(options.ngx.as_deref(), platform)?;
    // (AMD's FSR 4: MIT headers in the tree; HOOKSHOT_FSR=0 leaves the stubs)
    let fsr = fsr_headers(root).filter(|_| {
        std::env::var("HOOKSHOT_FSR")
            .ok()
            .as_deref()
            .is_none_or(|value| value != "0")
    });
    // (the Windows DLL uses llvm-mingw's clang/clang++, found exactly as the
    // Windows host link finds it; a missing one is a typed answer, not a
    // Python fallback)
    let (cc, cxx, link_cc, nm) = if platform == Platform::Windows {
        let toolchain = match toolchain.unwrap_or_else(crate::host_build::find_windows_toolchain) {
            Ok(toolchain) => toolchain,
            Err(error) => {
                return Err(VkBuildError::NoWindowsToolchain(format!(
                    "the Windows Vulkan back end cannot be built: {error}"
                )));
            }
        };
        let cxx = toolchain
            .clangxx
            .clone()
            .unwrap_or_else(|| toolchain.clang.clone());
        let nm = toolchain.clang.parent().map(|bin| bin.join("llvm-nm"));
        (
            toolchain.clang.to_string_lossy().into_owned(),
            cxx.to_string_lossy().into_owned(),
            toolchain.clang.to_string_lossy().into_owned(),
            nm.filter(|path| path.is_file()),
        )
    } else {
        let cc = crate::toolchain::find("clang", false)
            .to_string_lossy()
            .into_owned();
        let cxx = crate::toolchain::find("clang++", false)
            .to_string_lossy()
            .into_owned();
        (cc.clone(), cxx.clone(), cxx, None)
    };
    let vk = root.join("gpu/vk");
    let gpu = root.join("gpu");
    let zstd = root.join("third_party/zstd/lib");
    let imgui = root.join("third_party/imgui");
    let build = work.join("sa/build");
    let obj_dir = build.join("obj");
    let gen_dir = build.join("gen");
    let cache = work.join("build/objcache");
    for folder in [&obj_dir, &gen_dir] {
        fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    // (the start-up screen's logo and the overlays' fonts: generated here as
    // build.py's nx2/logo_rgba.py and nx2/ui_fonts.py make them)
    let logo = crate::logo_rgba::write(
        &gen_dir.join("hookshot_logo.h"),
        &root.join("assets/hookshot.png"),
    )?;
    let fonts = gen_dir.join("ui_fonts.cpp");
    crate::ui_fonts::write(&fonts, root)?;
    let stamp = shader_stamp(root)?;
    let release = release_requested();

    // (the dependency blobs, as build.py groups them: Eden and sirit see only
    // their own headers; the back end sees everything)
    let vk_lib_deps = tree_headers(&[
        vk.join("eden"),
        vk.join("sirit"),
        vk.join("compat"),
        gpu.join("compat"),
    ])?;
    let vdeps = tree_headers(&[
        vk.clone(),
        gpu.join("render"),
        gpu.join("compat"),
        gpu.join("third_party/suyu/video_core/textures"),
    ])?;
    let imgui_deps = flat_headers(&[imgui.clone(), imgui.join("backends"), zstd.clone()])?;
    let vdeps_imgui = {
        let mut digest = Sha1::new();
        digest.update(vdeps.as_bytes());
        digest.update(logo.as_bytes());
        digest.update(imgui_deps.as_bytes());
        format!("{:x}", digest.finalize())
    };
    let vdeps_ui = {
        let mut digest = Sha1::new();
        digest.update(vdeps.as_bytes());
        digest.update(imgui_deps.as_bytes());
        format!("{:x}", digest.finalize())
    };
    // (NVIDIA's and AMD's headers are that file's dependency alone: an SDK
    // update rebuilds vk_dlss.cpp or vk_fsr4.cpp and nothing else, as
    // build.py's ngx_deps and FSR flags do)
    let vdeps_ngx = match &ngx {
        Some(sdk) => Some(format!("{vdeps}{}", tree_headers(&[sdk.join("include")])?)),
        None => None,
    };
    let vdeps_fsr = match &fsr {
        Some(folder) => Some(format!(
            "{vdeps}{}",
            tree_headers(std::slice::from_ref(folder))?
        )),
        None => None,
    };
    let zstd_deps = zstd_headers(&zstd)?;
    let posix_deps = tree_headers(&[root.join("host/posix")])?;
    let deps = |name: &str| build.join(name);
    let vk_lib_deps_file = deps("vk_deps_eden.bin");
    let vdeps_file = deps("vk_deps.bin");
    let vdeps_imgui_file = deps("vk_deps_imgui.bin");
    let vdeps_ui_file = deps("vk_deps_ui.bin");
    let vdeps_ngx_file = deps("vk_deps_ngx.bin");
    let vdeps_fsr_file = deps("vk_deps_fsr.bin");
    let imgui_deps_file = deps("vk_deps_imgui_own.bin");
    let zstd_deps_file = deps("vk_deps_zstd.bin");
    let posix_deps_file = deps("vk_deps_posix.bin");
    for (path, text) in [
        (&vk_lib_deps_file, vk_lib_deps.as_str()),
        (&vdeps_file, vdeps.as_str()),
        (&vdeps_imgui_file, vdeps_imgui.as_str()),
        (&vdeps_ui_file, vdeps_ui.as_str()),
        (&vdeps_ngx_file, vdeps_ngx.as_deref().unwrap_or_default()),
        (&vdeps_fsr_file, vdeps_fsr.as_deref().unwrap_or_default()),
        (&imgui_deps_file, imgui_deps.as_str()),
        (&zstd_deps_file, zstd_deps.as_str()),
        (&posix_deps_file, posix_deps.as_str()),
    ] {
        write_deps(path, text)?;
    }

    let vflags = vk_flags_for(platform, root, &gen_dir, &roots, release);
    let imflags = imgui_flags(root);
    let mut jobs = Vec::new();
    // (Dear ImGui's five objects)
    let imgui_flags_here: Vec<String> = vflags.iter().chain(imflags.iter()).cloned().collect();
    for name in IMGUI_SOURCES {
        let src = imgui.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        let obj = obj_dir.join(format!(
            "vk_imgui_{}.o",
            name.rsplit('/')
                .next()
                .unwrap_or(name)
                .trim_end_matches(".cpp")
        ));
        jobs.push(job(
            &cxx,
            &src,
            &obj,
            &imgui_flags_here,
            &imgui_deps_file,
            &cache,
        ));
    }
    // (zstd's decoder)
    let zflags = zstd_flags_for(platform);
    for name in ZSTD_DECODER {
        let src = zstd.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        let obj = obj_dir.join(format!(
            "vk_zstd_{}.o",
            name.rsplit('/')
                .next()
                .unwrap_or(name)
                .trim_end_matches(".c")
        ));
        jobs.push(job(&cc, &src, &obj, &zflags, &zstd_deps_file, &cache));
    }
    // (the bundled fonts)
    jobs.push(job(
        &cxx,
        &fonts,
        &obj_dir.join("vk_ui_fonts.o"),
        &ui_fonts_flags_for(platform),
        Path::new(""),
        &cache,
    ));
    // (the back end's own sources)
    for src in sources(root)? {
        let relative = relative_to(&src, &vk);
        let name = src.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let lib = relative
            .components()
            .next()
            .is_some_and(|top| top.as_os_str() == "eden" || top.as_os_str() == "sirit");
        let deps_file = if lib {
            &vk_lib_deps_file
        } else if name == "vk_dlss.cpp" && vdeps_ngx.is_some() {
            &vdeps_ngx_file
        } else if name == "vk_fsr4.cpp" && vdeps_fsr.is_some() {
            &vdeps_fsr_file
        } else if IMGUI_USERS.contains(&name) {
            if name == "vk_imgui.cpp" {
                &vdeps_imgui_file
            } else {
                &vdeps_ui_file
            }
        } else {
            &vdeps_file
        };
        let mut flags = vflags.clone();
        if name == "vk_shader.cpp" {
            flags.push(shader_stamp_define(&stamp));
        }
        if name == "vk_menu.cpp" {
            // (the Insert menu's version line: build.py gives it this file only,
            // so a new release name rebuilds this one and nothing else)
            flags.push(crate::version::define(root));
        }
        if name == "vk_dlss.cpp"
            && let Some(sdk) = &ngx
        {
            flags.extend(ngx_flags(sdk));
        }
        if name == "vk_fsr4.cpp"
            && let Some(folder) = &fsr
        {
            flags.extend(fsr_flags(folder));
        }
        if IMGUI_USERS.contains(&name) {
            flags.extend(imflags.iter().cloned());
        }
        jobs.push(job(
            &cxx,
            &src,
            &obj_dir.join(object_name(&relative)),
            &flags,
            deps_file,
            &cache,
        ));
    }
    if platform == Platform::Windows
        && let Some(sdk) = &ngx
    {
        // (llvm-mingw: NVIDIA's static library was built with Microsoft's
        // compiler for its static runtime (/MT). vk_ngx_msvc_rt.cpp has its
        // C++ parts, host/mingw_msvc_shims.c the C ones the static FFmpeg
        // libraries share, as build.py compiles them)
        let extra = windows_ngx_sources(root);
        let mut shim_flags = vflags.clone();
        shim_flags.extend(ngx_flags(sdk));
        jobs.push(job(
            &cxx,
            &extra[0].1,
            &obj_dir.join(extra[0].0),
            &shim_flags,
            &vdeps_file,
            &cache,
        ));
        let mut mingw_flags = crate::host_build::mingw_target_flags();
        mingw_flags.extend([
            "-O2".to_owned(),
            "-g0".to_owned(),
            "-Wno-everything".to_owned(),
        ]);
        jobs.push(job(
            &cc,
            &extra[1].1,
            &obj_dir.join(extra[1].0),
            &mingw_flags,
            Path::new(""),
            &cache,
        ));
    }
    if platform == Platform::Linux {
        // (the Win32 calls on POSIX)
        jobs.push(job(
            &cc,
            &root.join("host/posix/win32_posix.c"),
            &obj_dir.join("vk_posix_win32_posix.o"),
            &posix_flags(root),
            &posix_deps_file,
            &cache,
        ));
    }

    let mut compiled =
        crate::compile::many_json(&json!({"jobs": jobs, "cache": cache, "parallel": 0}), root)?;
    if compiled["failed"]
        .as_array()
        .is_some_and(|failed| !failed.is_empty())
    {
        return Ok(json!({
            "stage": "vulkan-compile",
            "built": false,
            "reason": "compile",
            "compiled": compiled,
            "headers": roots.json(),
        }));
    }
    let objects: Vec<String> = jobs
        .iter()
        .filter_map(|job| job["obj"].as_str().map(str::to_owned))
        .collect();
    let library = build.join(platform.vulkan_library());
    let map = library.with_extension("map");
    let args = if platform == Platform::Windows {
        // (build.py's Windows link: llvm-mingw's native link flags, a DLL,
        // and the crash map beside it)
        let mut args = crate::host_build::mingw_target_flags();
        args.extend(crate::host_build::mingw_link_flags());
        args.extend([
            "-shared".to_owned(),
            "-o".to_owned(),
            library.to_string_lossy().into_owned(),
            format!("-Wl,-Xlink=/map:{}", map.display()),
        ]);
        args
    } else {
        vec![
            "-fuse-ld=lld".to_owned(),
            "-shared".to_owned(),
            "-o".to_owned(),
            library.to_string_lossy().into_owned(),
            "-Xlinker".to_owned(),
            format!("-Map={}.map", library.display()),
            "-Wl,-Bsymbolic".to_owned(),
            "-Wl,--gc-sections".to_owned(),
            "-static-libstdc++".to_owned(),
            "-static-libgcc".to_owned(),
        ]
    };
    let libs = link_libraries_for(platform, ngx.as_deref());
    let link = crate::linker::link_json(&json!({
        "cc": link_cc,
        "out": library.to_string_lossy(),
        "objects": objects,
        "args": args,
        "libs": libs,
    }))?;
    if !link["ok"].as_bool().unwrap_or(false) {
        return Ok(json!({
            "stage": "vulkan-link",
            "built": false,
            "reason": "link",
            "link": link,
            "compiled": compiled,
            "headers": roots.json(),
        }));
    }
    // (as build.py's check_vk_symbols: a symbol left for the loader means the
    // DLL would not load, and the only sign at run time is a black window.
    // On Windows llvm-nm -u must be empty too: lld resolves every import)
    let undefined = if platform == Platform::Windows {
        crate::linker::undefined_symbols_pe(&library, nm.as_deref())?
    } else {
        crate::linker::undefined_symbols(&library)?
    };
    if !undefined.is_empty() {
        return Ok(json!({
            "stage": "vulkan-symbols",
            "built": false,
            "reason": "symbols",
            "undefined": undefined,
            "link": link,
            "compiled": compiled,
            "headers": roots.json(),
        }));
    }
    let size = fs::metadata(&library).map(|meta| meta.len()).unwrap_or(0);
    compiled["library"] = json!(library.to_string_lossy());
    Ok(json!({
        "stage": "vulkan",
        "built": true,
        "platform": if platform == Platform::Windows { "windows" } else { "linux" },
        "library": library.to_string_lossy(),
        "map": map.to_string_lossy(),
        "size": size,
        "sources": sources(root)?.len(),
        "objects": objects.len(),
        "headers": roots.json(),
        "ngx": ngx.as_ref().map(|sdk| sdk.to_string_lossy().into_owned()),
        "fsr4": fsr.as_ref().map(|folder| folder.to_string_lossy().into_owned()),
        "compiled": compiled,
        "link": link,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("hookshot-rust-vk-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"// fixture\n").unwrap();
    }

    fn roots(folder: &Path) -> Roots {
        Roots {
            vulkan: folder.join("vulkan"),
            spirv: folder.join("spirv"),
            vma: folder.join("vma"),
        }
    }

    /// A repository-shaped root with the back end's sources and nothing else.
    fn sources_root(base: &Path) {
        for name in [
            "gpu/vk/vk_main.cpp",
            "gpu/vk/vk_shader.cpp",
            "gpu/vk/vk_ngx_msvc_rt.cpp",
            "gpu/vk/vk_internal.h",
            "gpu/vk/eden/shader_recompiler/frontend/ampere/lower_to_maxwell.cpp",
            "gpu/vk/eden/shader_recompiler/backend/spirv/emit_spirv.cpp",
            "gpu/vk/sirit/src/instructions.cpp",
            "gpu/vk/sirit/src/core.cpp",
            "gpu/third_party/suyu/video_core/textures/decoders.cpp",
            "gpu/third_party/suyu/video_core/textures/astc.cpp",
            "gpu/third_party/suyu/video_core/textures/texture.h",
            "gpu/third_party/suyu/video_core/buffer_cache.cpp",
        ] {
            touch(&base.join(name));
        }
    }

    #[test]
    fn sources_are_build_py_vk_sources() {
        let base = temp("sources");
        sources_root(&base);
        let found: Vec<String> = sources(&base)
            .unwrap()
            .iter()
            .map(|path| {
                path.strip_prefix(&base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            found,
            [
                "gpu/third_party/suyu/video_core/textures/astc.cpp",
                "gpu/third_party/suyu/video_core/textures/decoders.cpp",
                "gpu/vk/eden/shader_recompiler/backend/spirv/emit_spirv.cpp",
                "gpu/vk/eden/shader_recompiler/frontend/ampere/lower_to_maxwell.cpp",
                "gpu/vk/sirit/src/core.cpp",
                "gpu/vk/sirit/src/instructions.cpp",
                "gpu/vk/vk_main.cpp",
                "gpu/vk/vk_shader.cpp",
            ]
        );
        // (the NGX shim and suyu's other video_core sources are not the back end)
        assert!(
            !found
                .iter()
                .any(|name| name.ends_with("vk_ngx_msvc_rt.cpp"))
        );
        assert!(!found.iter().any(|name| name.ends_with("buffer_cache.cpp")));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn object_names_keep_build_pys_layout() {
        assert_eq!(
            object_name(Path::new(
                "eden/shader_recompiler/frontend/ampere/lower_to_maxwell.cpp"
            )),
            "vk_eden_shader_recompiler_frontend_ampere_lower_to_maxwell.o"
        );
        assert_eq!(object_name(Path::new("vk_main.cpp")), "vk_vk_main.o");
        // (suyu's texture decoders are outside gpu/vk: build.py's relpath
        // leaves the steps up out of it in the name)
        assert_eq!(
            object_name(Path::new(
                "../third_party/suyu/video_core/textures/astc.cpp"
            )),
            "vk_.._third_party_suyu_video_core_textures_astc.o"
        );
        assert_eq!(
            relative_to(
                Path::new("/repo/gpu/vk/eden/a.cpp"),
                Path::new("/repo/gpu/vk")
            ),
            Path::new("eden/a.cpp")
        );
        assert_eq!(
            relative_to(
                Path::new("/repo/gpu/third_party/suyu/textures/astc.cpp"),
                Path::new("/repo/gpu/vk")
            ),
            Path::new("../third_party/suyu/textures/astc.cpp")
        );
    }

    #[test]
    fn flags_carry_build_pys_defines_and_include_order() {
        // (the include folders are compared as POSIX paths, as build.py writes
        // them; the port itself only builds the Linux shared object)
        if Platform::current() != Platform::Linux {
            return;
        }
        let base = temp("flags");
        let vk = base.join("gpu/vk");
        let posix = base.join("host/posix");
        let suyu = base.join("gpu/third_party/suyu");
        let headers = roots(&base);
        let generated = base.join("work/sa/build/gen");
        for path in [
            vk.join("compat"),
            base.join("gpu/compat"),
            vk.join("eden"),
            suyu,
            vk.join("sirit/include"),
            vk.join("sirit/src"),
            headers.spirv.join("include"),
            headers.spirv.join("include/spirv/unified1"),
            headers.vulkan.join("include"),
            headers.vma.join("include"),
            vk.clone(),
            generated.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let flags = vk_flags(&base, &generated, &headers, false);
        for wanted in [
            "-std=c++20",
            "-O2",
            "-DNDEBUG",
            "-Wno-everything",
            "-DVK_NO_PROTOTYPES",
            "-DMKWVK_BUILDING",
            "-DMKWVK_INCLUDES=2",
            "-fPIC",
            "-D_GNU_SOURCE",
            "-include",
            "vector",
        ] {
            assert!(flags.iter().any(|flag| flag == wanted), "{wanted}");
        }
        assert!(!flags.iter().any(|flag| flag == "-DHOOKSHOT_RELEASE"));
        assert!(
            vk_flags(&base, &generated, &headers, true)
                .iter()
                .any(|flag| flag == "-DHOOKSHOT_RELEASE")
        );
        // (Eden's recompiler before suyu's root, and the four header roots
        // after sirit, exactly as build.py orders them: -DMKWVK_INCLUDES=2
        // says so with the include folders left out of object keys)
        let includes: Vec<&str> = flags
            .iter()
            .filter(|flag| flag.starts_with("-I"))
            .map(String::as_str)
            .collect();
        let find = |suffix: &str| {
            includes
                .iter()
                .position(|path| path.ends_with(suffix))
                .unwrap_or_else(|| panic!("{suffix} is missing from {includes:?}"))
        };
        assert_eq!(find("host/posix"), 0);
        let compat = find("gpu/vk/compat");
        assert_eq!(find("gpu/compat"), compat + 1);
        assert_eq!(find("gpu/vk/eden"), compat + 2);
        assert_eq!(find("gpu/third_party/suyu"), compat + 3);
        assert_eq!(find("gpu/vk/sirit/include"), compat + 4);
        assert_eq!(find("gpu/vk/sirit/src"), compat + 5);
        assert!(find("spirv/include") < find("spirv/include/spirv/unified1"));
        assert!(find("spirv/include/spirv/unified1") < find("vulkan/include"));
        assert!(find("vulkan/include") < find("vma/include"));
        assert!(find("work/sa/build/gen") > find("gpu/vk"));
        // (the tests' header roots are not this repository's)
        let _ = posix;
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn the_shader_stamp_ignores_line_endings_and_follows_the_translator() {
        let base = temp("stamp");
        let vk = base.join("gpu/vk");
        touch(&vk.join("eden/a.cpp"));
        touch(&vk.join("sirit/b.cpp"));
        touch(&vk.join("compat/c.h"));
        for name in ["vk_shader.cpp", "vk_bank.cpp", "vk_state.h", "vk_api.h"] {
            touch(&vk.join(name));
        }
        let first = shader_stamp(&base).unwrap();
        assert_eq!(first.len(), 16);
        assert_eq!(shader_stamp(&base).unwrap(), first);
        fs::write(vk.join("eden/a.cpp"), b"// one\r\n// two\r\n").unwrap();
        let crlf = shader_stamp(&base).unwrap();
        fs::write(vk.join("eden/a.cpp"), b"// one\n// two\n").unwrap();
        assert_eq!(shader_stamp(&base).unwrap(), crlf);
        fs::write(vk.join("eden/a.cpp"), b"// changed\n").unwrap();
        assert_ne!(shader_stamp(&base).unwrap(), crlf);
        assert_eq!(
            shader_stamp_define(&first),
            format!("-DHOOKSHOT_SHADER_STAMP=0x{first}ull")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn object_keys_are_stable_and_follow_the_shader_stamp() {
        let base = temp("keys");
        let source = base.join("vk_shader.cpp");
        fs::write(&source, b"int hookshot_vk_key(void) { return 1; }\n").unwrap();
        let headers = roots(&base);
        let generated = base.join("gen");
        let flags = vk_flags(&base, &generated, &headers, false);
        let first = crate::compile::object_key(&source, &flags, b"headers", &base).unwrap();
        assert_eq!(
            crate::compile::object_key(&source, &flags, b"headers", &base).unwrap(),
            first
        );
        let mut stamped = flags.clone();
        stamped.push(shader_stamp_define("0123456789abcdef"));
        let one = crate::compile::object_key(&source, &stamped, b"headers", &base).unwrap();
        assert_eq!(
            crate::compile::object_key(&source, &stamped, b"headers", &base).unwrap(),
            one
        );
        assert_ne!(one, first);
        let mut other = stamped.clone();
        *other.last_mut().unwrap() = shader_stamp_define("fedcba9876543210");
        assert_ne!(
            crate::compile::object_key(&source, &other, b"headers", &base).unwrap(),
            one
        );
        assert_ne!(
            crate::compile::object_key(&source, &stamped, b"other headers", &base).unwrap(),
            one
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn header_roots_prefer_the_environment_then_the_system() {
        let base = temp("headers");
        let system = base.join("system");
        let env = base.join("env/include");
        touch(&system.join("include/vulkan/vulkan.h"));
        touch(&system.join("include/spirv/unified1/spirv.h"));
        touch(&system.join("include/vk_mem_alloc.h"));
        touch(&env.join("vulkan/vulkan.h"));
        let search = HeaderSearch {
            system: vec![system.clone()],
            cpm: vec![],
        };
        let found = Headers::find(
            &search,
            &HeaderEnv {
                vulkan: Some(env.clone()),
                ..HeaderEnv::default()
            },
        );
        // (the environment names the include folder itself)
        assert_eq!(found.vulkan, Some(base.join("env")));
        assert_eq!(found.spirv, Some(system.clone()));
        assert_eq!(found.vma, Some(system));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn missing_headers_are_reported_with_build_pys_guidance() {
        let search = HeaderSearch {
            system: vec![],
            cpm: vec![],
        };
        let found = Headers::find(&search, &HeaderEnv::default());
        assert_eq!(found.missing(), ["Vulkan", "SPIR-V", "VMA"]);
        assert!(found.roots().is_none());
        let answer = skipped(&found.missing());
        assert_eq!(answer["built"], false);
        assert_eq!(answer["reason"], "headers");
        let message = answer["message"].as_str().unwrap();
        let library = format!("no headers for {}", Platform::current().vulkan_library());
        assert!(message.contains(&library), "{library} in {message}");
        for wanted in [
            "RX_CPM",
            "HOOKSHOT_VULKAN_HEADERS",
            "HOOKSHOT_SPIRV_HEADERS",
            "HOOKSHOT_VMA_HEADERS",
            "vulkan-headers + spirv-headers + vulkan-memory-allocator",
            "HOOKSHOT_VK_PREBUILT",
        ] {
            assert!(message.contains(wanted), "{wanted} in {message}");
        }
    }

    #[test]
    fn a_build_without_headers_skips_instead_of_failing() {
        if Platform::current() != Platform::Linux {
            return;
        }
        let base = temp("skip");
        let work = base.join("work");
        let answer = build_at_with(
            &base,
            &work,
            &HeaderSearch {
                system: vec![],
                cpm: vec![],
            },
            &HeaderEnv::default(),
        )
        .unwrap();
        assert_eq!(answer["stage"], "vulkan");
        assert_eq!(answer["built"], false);
        assert_eq!(answer["reason"], "headers");
        assert!(!work.join("sa/build/hookshot_vk.so").exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn header_search_finds_a_cpm_cache_last() {
        let base = temp("cpm");
        let cache = base.join("cpm");
        let package = cache.join("vulkanheaders/v1.4.0");
        touch(&package.join("include/vulkan/vulkan.h"));
        let search = HeaderSearch {
            system: vec![],
            cpm: vec![cache],
        };
        let found = header_package(&search, "vulkanheaders", "vulkan/vulkan.h", None);
        assert_eq!(found, Some(package));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn ngx_flags_name_the_sdk_and_the_link_puts_it_before_the_system_libs() {
        let base = temp("ngx");
        let sdk = base.join("sdk");
        let platform = crate::ngx_sdk::Platform::current();
        touch(&sdk.join("include/nvsdk_ngx.h"));
        touch(&crate::ngx_sdk::library(&sdk, platform));
        let resolved = resolve_ngx(Some(&sdk)).unwrap().unwrap();
        assert_eq!(resolved, sdk);
        let flags = ngx_flags(&resolved);
        assert!(flags.iter().any(|flag| flag == "-DHOOKSHOT_NGX"));
        assert!(
            flags
                .iter()
                .any(|flag| flag == &format!("-I{}", sdk.join("include").display()))
        );
        let libs = link_libraries(Some(&resolved));
        let expected = crate::ngx_sdk::library(&sdk, platform)
            .to_string_lossy()
            .into_owned();
        // (the Windows link puts the platform's libraries first and NVIDIA's
        // static library after them; the Linux link names it first)
        match platform {
            crate::ngx_sdk::Platform::Linux => {
                assert_eq!(libs[0], expected);
                assert_eq!(&libs[1..], ["-ldl", "-lpthread", "-lm"]);
            }
            crate::ngx_sdk::Platform::Windows => {
                assert_eq!(&libs[..3], ["-luser32", "-lc++", "-lunwind"]);
                assert_eq!(libs[3], expected);
                assert_eq!(
                    &libs[4..],
                    ["-ladvapi32", "-Wl,-Xlink=/nodefaultlib:libcpmt"]
                );
            }
        }
        // (a folder without the library is an error, not a silent stub build)
        let broken = base.join("broken");
        fs::create_dir_all(&broken).unwrap();
        let error = resolve_ngx(Some(&broken)).unwrap_err();
        assert!(error.contains("has no"), "{error}");
        assert!(error.contains("Unset HOOKSHOT_NGX_SDK"), "{error}");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn vsync_auto_follows_the_runtimes_frame_limit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let header = fs::read_to_string(root.join("gpu/vk/vk_dlss.h")).unwrap();
        let dlss = fs::read_to_string(root.join("gpu/vk/vk_dlss.cpp")).unwrap();
        let main = fs::read_to_string(root.join("gpu/vk/vk_main.cpp")).unwrap();
        assert!(header.contains("bool dlss_fg_runs(int frames);"));
        assert!(dlss.contains("std::atomic<unsigned> most_known{0};"));
        assert!(dlss.contains("RX_VK_DLSS_FG_MOST"));
        assert!(dlss.contains("g.most_known = g.most;"));
        assert!(dlss.contains("return !most || frames <= (int)most;"));
        assert!(main.contains("dlss_fg_runs(opts.framegen - 1)"));
        assert!(main.contains("its frames the renderer's own"));
        // (the no-NGX build keeps the same signature)
        assert!(dlss.contains("bool dlss_fg_runs(int) { return false; }"));
    }

    #[test]
    fn a_failed_dlss_feature_is_retried_after_a_setting_changes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dlss = fs::read_to_string(root.join("gpu/vk/vk_dlss.cpp")).unwrap();
        assert!(dlss.contains("uint64_t refused = 0;"));
        assert!(
            dlss.contains("if (s.refused == key) return *fail = \"its feature was not made\", 0;")
        );
        assert!(dlss.contains("s.refused = key;"));
        assert!(dlss.contains("s.refused = 0;"));
    }

    #[test]
    fn noperspective_varyings_are_scaled_and_undone() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let info =
            fs::read_to_string(root.join("gpu/vk/eden/shader_recompiler/runtime_info.h")).unwrap();
        let context = fs::read_to_string(
            root.join("gpu/vk/eden/shader_recompiler/backend/spirv/spirv_emit_context.cpp"),
        )
        .unwrap();
        let special = fs::read_to_string(
            root.join("gpu/vk/eden/shader_recompiler/backend/spirv/emit_spirv_special.cpp"),
        )
        .unwrap();
        let getset = fs::read_to_string(
            root.join("gpu/vk/eden/shader_recompiler/backend/spirv/emit_spirv_context_get_set.cpp"),
        )
        .unwrap();
        let stream = fs::read_to_string(root.join("gpu/vk/sirit/src/stream.h")).unwrap();
        let shader = fs::read_to_string(root.join("gpu/vk/vk_shader.cpp")).unwrap();
        assert!(info.contains("std::bitset<32> noperspective_outputs{};"));
        assert!(info.contains("bool scaled_noperspective_inputs{};"));
        assert!(special.contains("ScaleNoPerspectiveOutputs"));
        assert!(special.contains("RestoreNoPerspectiveOutputs"));
        assert!(special.contains("ctx.OpVectorTimesScalar(type, value, w)"));
        assert!(
            special.contains("ctx.stage == Stage::VertexB || ctx.stage == Stage::TessellationEval")
        );
        assert!(getset.contains("ctx.noperspective_as_smooth_inputs[index]"));
        assert!(getset.contains("ctx.OpFMul(ctx.F32[1], value, frag_w)"));
        assert!(context.contains("noperspective_as_smooth_inputs[index] = true;"));
        assert!(context.contains("runtime_info.scaled_noperspective_inputs"));
        assert!(stream.contains(
            "throw std::runtime_error(\"sirit: an operand with no SPIR-V definition (null Id)\");"
        ));
        assert!(shader.contains("loads_indexed_attributes"));
        assert!(shader.contains("vrt.xfb_count == 0"));
        assert!(shader.contains("RX_VK_LOWER_SKIP"));
    }

    #[test]
    fn retired_imports_use_the_grace_rule() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mem = fs::read_to_string(root.join("gpu/vk/vk_memory.cpp")).unwrap();
        for needle in [
            "#include \"retired_give_back.h\"",
            "retired_give_back::mode(v && *v, env_on(\"RX_VK_KEEP_RETIRED\"), windows, dev.props.vendorID == 0x10DE)",
            "retired_give_back::due(mode, idle, idle_ms, floor_ms, want > 0)",
            "env_int(\"RX_VK_RETIRE_SECS\", 30)",
            "env_int(\"RX_VK_RETIRE_MIN_SECS\", 2)",
            "if (mode == Mode::GRACE && high > 0) {",
            "old->retired_ms = import_now_ms();",
            "retired_give_back::settled(used, dead_import_bytes), budget, high, low)",
            "if (retired_mode() == retired_give_back::Mode::GRACE) used = retired_give_back::settled(used, dead_import_bytes);",
            "dead_import_bytes += p.hi - p.lo;",
            "dead_import_bytes -= std::min(dead_import_bytes, dead_imports.front().bytes);",
        ] {
            assert!(mem.contains(needle), "vk_memory.cpp no longer has {needle}");
        }
    }

    #[test]
    fn a_failed_dlss_frame_is_retried_and_a_menu_change_clears_the_latch() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let checks = fs::read_to_string(root.join("gpu/vk/dlss_checks.h")).unwrap();
        let temporal = fs::read_to_string(root.join("gpu/vk/vk_temporal.cpp")).unwrap();
        let menu = fs::read_to_string(root.join("gpu/vk/vk_menu.cpp")).unwrap();
        assert!(checks.contains("(uint64_t)(w + 2) * 3 >= ow"));
        assert!(temporal.contains("void temporal_reset_failure()"));
        assert!(temporal.contains("if (engine.e->retry)"));
        assert!(menu.contains("temporal_reset_failure();"));
    }

    #[test]
    fn backend_init_failures_name_their_step() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let main = fs::read_to_string(root.join("gpu/vk/vk_main.cpp")).unwrap();
        let backend = fs::read_to_string(root.join("host/rx_backend.c")).unwrap();
        assert!(main.contains("vk: create_instance failed"));
        assert!(main.contains("vk: create_device failed"));
        assert!(backend.contains("has no mkwvk_init: WebGPU"));
        assert!(backend.contains("mkwvk_init version mismatch"));
    }

    #[test]
    fn reflex_tags_each_submit_with_its_game_frame() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let reflex = fs::read_to_string(root.join("gpu/vk/vk_reflex.cpp")).unwrap();
        let replay = fs::read_to_string(root.join("gpu/vk/vk_replay.cpp")).unwrap();
        assert!(reflex.contains("const void* reflex_submit_id(const void* next, uint64_t frame)"));
        assert!(reflex.contains("RX_VK_REFLEX_SUBMIT_IDS"));
        assert_eq!(replay.matches("si.pNext = reflex_submit_id(").count(), 4);
        assert!(replay.contains("pend.reflex = reflex_frame()"));
    }

    #[test]
    fn the_win32_audio_ring_drops_the_oldest_buffer() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let posix = fs::read_to_string(root.join("host/posix/win32_posix.c")).unwrap();
        assert!(posix.contains("if (w->count >= 60)"));
        assert!(posix.contains("w->head = (w->head + 1) % 64;"));
        assert!(posix.contains("WHDR_INQUEUE) | WHDR_DONE"));
    }

    #[test]
    fn esc_opens_the_menu_by_default_after_the_version_five_move() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let header = fs::read_to_string(root.join("gpu/vk/vk_menu.h")).unwrap();
        let menu = fs::read_to_string(root.join("gpu/vk/vk_menu.cpp")).unwrap();
        assert!(header.contains("int menu_shortcut = 2;"));
        assert!(menu.contains("options_version=5"));
        assert!(
            menu.contains("if (version < 5 && opts.menu_shortcut == 0) opts.menu_shortcut = 2;")
        );
    }

    #[test]
    fn the_graphics_tab_is_grouped_under_section_titles() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let menu = fs::read_to_string(root.join("gpu/vk/vk_menu.cpp")).unwrap();
        let table = &menu[menu.find("Row rows[] = {").unwrap()..];
        let table = &table[..table.find("\n};").unwrap()];
        // (tab, name, value expression, heading: a heading's name is its section's title)
        let rows: Vec<(&str, String, String, bool)> = table
            .split("\n    {TAB_")
            .skip(1)
            .map(|entry| {
                let tab = &entry[..entry.find(',').unwrap()];
                let q1 = entry.find('"').unwrap();
                let q2 = q1 + 1 + entry[q1 + 1..].find('"').unwrap();
                let name = entry[q1 + 1..q2].to_string();
                let after = &entry[q2 + 1..];
                let value = after[after.find(',').unwrap() + 1..]
                    .trim_start()
                    .split(',')
                    .next()
                    .unwrap()
                    .trim()
                    .to_string();
                let heading = entry.contains(
                    "nullptr, 0, 0, 0, nullptr, nullptr, nullptr, false, true, -1, true}",
                );
                (tab, name, value, heading)
            })
            .collect();
        assert!(rows.len() >= 40, "the rows table was not found");

        // (the sections the tab draws, in order, each with the rows it holds)
        let sections = [
            (
                "Resolution and upscaling",
                &[
                    "Output resolution",
                    "Profile",
                    "Upscaler",
                    "DLSS model",
                    "Render resolution",
                    "Sharpness",
                ][..],
            ),
            (
                "Frame generation and latency",
                &[
                    "Frame generation",
                    "Frame generation by",
                    "Frame pacing",
                    "Game frame rate cap",
                    "NVIDIA Reflex",
                    "AMD Anti-Lag 2",
                ][..],
            ),
            (
                "Screen",
                &[
                    "Fullscreen",
                    "V-Sync",
                    "Window scaling filter",
                    "Ultrawide",
                    "Ultrawide menus",
                ][..],
            ),
            (
                "HDR",
                &[
                    "HDR",
                    "HDR paper white",
                    "HDR peak brightness",
                    "HDR contrast",
                ][..],
            ),
            ("Detail", &["Anisotropic filtering"][..]),
            (
                "GPU and memory",
                &["GPU device", "VRAM budget", "Mesh shaders"][..],
            ),
        ];
        let graphics: Vec<_> = rows.iter().filter(|row| row.0 == "GRAPHICS").collect();
        assert_eq!(
            graphics[0].1, "Quality preset",
            "the Graphics tab no longer starts with the Quality preset"
        );
        assert!(!graphics[0].3);
        let mut want = vec!["Quality preset".to_string()];
        for (title, members) in sections {
            want.push(format!("[{title}]"));
            want.extend(members.iter().map(|name| name.to_string()));
        }
        let got: Vec<String> = graphics
            .iter()
            .map(|row| {
                if row.3 {
                    format!("[{}]", row.1)
                } else {
                    row.1.clone()
                }
            })
            .collect();
        assert_eq!(got, want, "the Graphics tab's sections and rows moved");

        // (a heading draws as a title and the selection steps over it)
        let function = |marker: &str| {
            let start = menu.find(marker).unwrap();
            let mut depth = 0;
            for (at, c) in menu[start..].char_indices() {
                if c == '{' {
                    depth += 1;
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        return &menu[start..start + at + 1];
                    }
                }
            }
            panic!("unterminated {marker}");
        };
        let draw = function("void draw_options(ui::Ctx& c) {");
        assert!(
            draw.contains("if (r.heading) {"),
            "heading rows are no longer drawn as titles"
        );
        let tick = function("void menu_tick() {");
        assert!(
            tick.contains("while (heading_at(selected));"),
            "the keys no longer step over heading rows"
        );

        // (no section can be left with only rows row_shown hides: a title over nothing on a GPU without them)
        let shown = function("bool row_shown(int i) {");
        let mut hidable = Vec::new();
        let mut at = 0;
        while let Some(found) = shown[at..].find("&opts.") {
            let start = at + found;
            let end = shown[start + 1..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
                .map(|end| start + 1 + end)
                .unwrap_or(shown.len());
            hidable.push(&shown[start..end]);
            at = end;
        }
        assert!(hidable.contains(&"&opts.reflex") && hidable.contains(&"&opts.hdr_white"));
        for (i, heading) in rows.iter().enumerate().filter(|(_, row)| row.3) {
            let section: Vec<_> = rows[i + 1..]
                .iter()
                .take_while(|row| row.0 == heading.0 && !row.3)
                .collect();
            assert!(
                !section.is_empty(),
                "{}: the section {} has no rows",
                heading.0,
                heading.1
            );
            assert!(
                section.iter().any(|row| !hidable.contains(&row.2.as_str())),
                "{}: every row of the section {} can be hidden (row_shown)",
                heading.0,
                heading.1
            );
        }
    }

    #[test]
    fn radv_and_the_xtxh_navi_21_ids_are_out_of_the_amd_workarounds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let draw = fs::read_to_string(root.join("gpu/vk/vk_draw.cpp")).unwrap();
        let main = fs::read_to_string(root.join("gpu/vk/vk_main.cpp")).unwrap();
        assert!(
            draw.contains(
                "dev.props.vendorID == 0x1002 && dev.driver_id != VK_DRIVER_ID_MESA_RADV"
            )
        );
        assert!(main.contains("dev.props.deviceID == 0x73af"));
        assert!(main.contains("dev.props.deviceID == 0x73a5"));
    }

    #[test]
    fn gcn_gets_no_mirrors_batching_or_f16() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let memory = fs::read_to_string(root.join("gpu/vk/vk_memory.cpp")).unwrap();
        let replay = fs::read_to_string(root.join("gpu/vk/vk_replay.cpp")).unwrap();
        let shader = fs::read_to_string(root.join("gpu/vk/vk_shader.cpp")).unwrap();
        assert!(
            memory.contains("const bool gcn = dev.sg32_stages == 0 && dev.subgroup_size > 32;")
        );
        assert!(memory.contains("discrete && !gcn"));
        assert!(replay.contains("radv && !gcn"));
        assert!(shader.contains("!env_on(\"RX_VK_NO_F16\") && dev.vk12.shaderFloat16"));
        assert!(shader.contains("0x6e663136u"));
        assert!(shader.contains("!dev.vk12.shaderFloat16"));
    }

    #[test]
    fn image_pool_blocks_are_made_at_the_start() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let vma = fs::read_to_string(root.join("gpu/vk/vk_vma.cpp")).unwrap();
        assert!(vma.contains("RX_VK_IMAGE_PREALLOC"));
        assert!(vma.contains("pi.minBlockCount = (size_t)n;"));
        assert!(vma.contains("heap / 100 * 15 / kImageBlock"));
    }

    #[test]
    fn gpu_written_mirrors_are_not_given_back_empty() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mem = fs::read_to_string(root.join("gpu/vk/vk_memory.cpp")).unwrap();
        assert!(mem.contains("bool gpu_only = false;"));
        assert!(mem.contains("bool gpu_written = false;"));
        assert!(
            mem.contains(
                "p.gpu_only |= o->second.gpu_only, p.gpu_written |= o->second.gpu_written"
            )
        );
        assert!(mem.contains("if (it->second.hi > guest) it->second.gpu_only = true;"));
        assert!(mem.contains("p->gpu_written = true;"));
        assert!(mem.contains("if (p.gpu_only || p.gpu_written) kept_bytes"));
        assert!(mem.contains("inherit_lost"));
    }

    #[test]
    fn background_workers_use_more_of_a_16_thread_cpu() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let jobs = fs::read_to_string(root.join("gpu/vk/vk_jobs.cpp")).unwrap();
        assert!(jobs.contains("hw >= 16 ? 6u"));
        assert!(jobs.contains("RX_VK_WORKERS"));
        assert!(jobs.contains("std::clamp(atoi(e), 1, 16)"));
    }

    #[test]
    fn windows_ngx_libraries_keep_their_order() {
        let sdk = PathBuf::from("C:/ngx-sdk");
        let libs = link_libraries_for(Platform::Windows, Some(&sdk));
        assert_eq!(&libs[..3], ["-luser32", "-lc++", "-lunwind"]);
        assert_eq!(
            PathBuf::from(&libs[3]),
            crate::ngx_sdk::library(&sdk, crate::ngx_sdk::Platform::Windows)
        );
        assert_eq!(
            &libs[4..],
            ["-ladvapi32", "-Wl,-Xlink=/nodefaultlib:libcpmt"]
        );
        assert_eq!(
            link_libraries_for(Platform::Windows, None),
            ["-luser32", "-lc++", "-lunwind"]
        );
    }

    #[test]
    fn windows_flags_follow_build_pys_windows_branch() {
        let base = temp("windows-flags");
        let headers = roots(&base);
        let generated = base.join("work/sa/build/gen");
        let flags = vk_flags_for(Platform::Windows, &base, &generated, &headers, false);
        for wanted in [
            "--target=x86_64-w64-mingw32",
            "-fms-extensions",
            "-D_CRT_SECURE_NO_WARNINGS",
            "-std=c++20",
            "-O2",
            "-DNDEBUG",
            "-Wno-everything",
            "-DVK_NO_PROTOTYPES",
            "-DMKWVK_BUILDING",
            "-DMKWVK_INCLUDES=2",
        ] {
            assert!(flags.iter().any(|flag| flag == wanted), "{wanted} missing");
        }
        for unwanted in ["-fPIC", "-D_GNU_SOURCE"] {
            assert!(!flags.iter().any(|flag| flag == unwanted), "{unwanted}");
        }
        assert!(
            !flags
                .iter()
                .any(|flag| flag.contains("host/posix") || flag.contains("host\\posix")),
            "the POSIX shim is Linux's"
        );
        assert!(flags.iter().any(|flag| flag.starts_with("-march=")));
        // (zstd and the fonts use the same target, without -fPIC)
        for flags in [
            zstd_flags_for(Platform::Windows),
            ui_fonts_flags_for(Platform::Windows),
        ] {
            assert!(
                flags
                    .iter()
                    .any(|flag| flag == "--target=x86_64-w64-mingw32")
            );
            assert!(!flags.iter().any(|flag| flag == "-fPIC"));
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_ngx_jobs_add_the_msvc_runtime_shims_and_their_libraries() {
        let base = temp("windows-ngx");
        let extra = windows_ngx_sources(&base);
        assert_eq!(extra[0].0, "vk_ngx_msvc_rt.o");
        assert_eq!(extra[0].1, base.join("gpu/vk/vk_ngx_msvc_rt.cpp"));
        assert_eq!(extra[1].0, "vk_ngx_mingw_msvc_shims.o");
        assert_eq!(extra[1].1, base.join("host/mingw_msvc_shims.c"));
        // (the regular source list never compiles the NGX shim itself)
        assert!(
            !sources(&base)
                .unwrap()
                .iter()
                .any(|path| path.ends_with("vk_ngx_msvc_rt.cpp"))
        );

        let sdk = base.join("sdk");
        let library = crate::ngx_sdk::library(&sdk, crate::ngx_sdk::Platform::Windows);
        let libs = link_libraries_for(Platform::Windows, Some(&sdk));
        assert_eq!(
            &libs[..3],
            vec![
                "-luser32".to_owned(),
                "-lc++".to_owned(),
                "-lunwind".to_owned()
            ]
        );
        assert_eq!(PathBuf::from(&libs[3]), library);
        assert_eq!(libs[4], "-ladvapi32");
        assert!(libs[5].contains("/nodefaultlib:libcpmt"));
        assert!(!libs.iter().any(|lib| lib == "-ldl"));
        // (without DLSS, no NVIDIA library, advapi32 or libcpmt option)
        assert_eq!(
            link_libraries_for(Platform::Windows, None),
            vec![
                "-luser32".to_owned(),
                "-lc++".to_owned(),
                "-lunwind".to_owned()
            ]
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_missing_windows_toolchain_is_a_typed_error() {
        let base = temp("windows-no-toolchain");
        let system = base.join("system");
        for path in [
            "include/vulkan/vulkan.h",
            "include/spirv/unified1/spirv.h",
            "include/vk_mem_alloc.h",
        ] {
            touch(&system.join(path));
        }
        // (the environment names the headers: this PC's /usr is not searched
        // for a PE compile, so the system entry alone would be skipped)
        let environment = HeaderEnv {
            vulkan: Some(system.clone()),
            spirv: Some(system.clone()),
            vma: Some(system.clone()),
        };
        let search = HeaderSearch {
            system: vec![system],
            cpm: vec![],
        };
        let error = build_at_for(
            Platform::Windows,
            &base,
            &base.join("work"),
            &search,
            &environment,
            &Options::default(),
            Some(Err("no llvm-mingw was found: test".into())),
        )
        .unwrap_err();
        assert!(matches!(error, VkBuildError::NoWindowsToolchain(_)));
        let message = error.to_string();
        assert!(message.contains("llvm-mingw"), "{message}");
        assert!(message.contains("test"), "{message}");
        // (a header set the caller does not have is the same skip on Windows,
        // not a crash or a Python fallback)
        let missing = build_at_for(
            Platform::Windows,
            &base,
            &base.join("work"),
            &HeaderSearch {
                system: vec![],
                cpm: vec![],
            },
            &HeaderEnv::default(),
            &Options::default(),
            None,
        )
        .unwrap();
        assert_eq!(missing["built"], false);
        assert_eq!(missing["reason"], "headers");
        assert!(
            missing["message"]
                .as_str()
                .unwrap()
                .contains("hookshot_vk.dll")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn fsr_flags_follow_the_vendored_headers() {
        let base = temp("fsr");
        assert_eq!(fsr_headers(&base), None);
        touch(&base.join("third_party/fidelityfx/api/include/ffx_api.h"));
        let folder = fsr_headers(&base).unwrap();
        assert_eq!(folder, base.join("third_party/fidelityfx"));
        let flags = fsr_flags(&folder);
        assert!(flags.iter().any(|flag| flag == "-DHOOKSHOT_FSR"));
        assert!(
            flags
                .iter()
                .any(|flag| flag == &format!("-I{}", folder.display()))
        );
        let _ = fs::remove_dir_all(base);
    }

    fn function<'a>(source: &'a str, head: &str) -> &'a str {
        let start = source
            .find(head)
            .unwrap_or_else(|| panic!("{head} is gone from the source"));
        let body = &source[start..];
        &body[..body.find("\n}\n").unwrap_or(body.len())]
    }

    #[test]
    fn mesh_fallback_pipelines_need_no_dynamic_vertex_stride() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let draw = fs::read_to_string(root.join("gpu/vk/vk_draw.cpp")).unwrap();
        // (the rule: ordinary vertex input asks for the dynamic stride, the fallback and native mesh do not)
        assert!(draw.contains("fallback_pipeline_state::dynamic_input_assembly(p.mesh)"));
        assert!(draw.contains("fallback_pipeline_state::dynamic_vertex_stride(p.mesh, p.fallback)"));
        // (a fallback bind invalidates the dynamic strides: both skip caches are dropped, and the next ordinary draw rebinds)
        assert!(draw.contains(
            "fallback_pipeline_state::after_graphics_bind(p.fallback, last_vb_valid, vertex_set.on);"
        ));
        let policy = fs::read_to_string(root.join("gpu/vk/fallback_pipeline_state.h")).unwrap();
        let bind = function(&policy, "inline void after_graphics_bind(");
        assert!(bind.contains("last_vb_valid = false;"));
        assert!(bind.contains("vertex_set_on = false;"));
    }

    #[test]
    fn mesh_fallback_programs_in_memory_get_their_compute_pipelines_before_the_title() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cache = fs::read_to_string(root.join("gpu/vk/vk_progcache.cpp")).unwrap();
        let load = function(&cache, "size_t program_cache_load_some(");
        // (after the programs read in now were given, under the lock on the programs: a key read in now is given once)
        let lock = load.find("std::lock_guard lk(shader_mu);").unwrap();
        let now = load.find("fn(*p);").unwrap();
        let earlier = load.find("fallback_loaded::each(programs, given, fn)").unwrap();
        assert!(lock < now && now < earlier, "the fallback rule moved before the programs read in now");
        assert!(load.contains("fallback_loaded::on(getenv(\"RX_VK_FALLBACK_LOADED\"))"));
        assert!(load.contains("if (fallbacks && loaded_too) {"));
        assert!(load.contains("for (const auto& w : work) given.insert(w.first);"));
        // (the precompile asks for every fallback program and makes its compute pipelines before play)
        let draw = fs::read_to_string(root.join("gpu/vk/vk_draw.cpp")).unwrap();
        let precompile = function(&draw, "void precompile_run(void (*progress)(size_t done, size_t total)) {");
        assert!(precompile.contains("else if (p.fallback) fallbacks.push_back(&p);"));
        assert!(precompile.contains("fallback_compute_pipelines(p);"));
        assert!(precompile.contains("}, all_computes, true);"));
    }

    #[test]
    fn the_load_default_safety_net_counts_only_cache_misses() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let rule = fs::read_to_string(root.join("gpu/vk/load_default.h")).unwrap();
        assert!(rule.contains("constexpr long long kNotInCacheMost = 500;"));
        let rule = function(&rule, "inline bool caches_lacked(");
        assert!(rule.contains("long long listed_not_in_a_cache"));
        // (draws left out and pipeline waits are not this mode's alone: a state no list names is left out in start mode too)
        for other in ["left_out", "pipeline_wait", "pskip", "pwait"] {
            assert!(!rule.contains(other), "the safety net counts {other} again");
        }
    }

    #[test]
    fn a_lost_device_asks_for_the_fault_info_without_a_switch_off_nvidia() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let draw = fs::read_to_string(root.join("gpu/vk/vk_draw.cpp")).unwrap();
        assert!(draw.contains("device_fault_query::forced(getenv(\"RX_VK_FAULT_INFO\"))"));
        assert!(draw.contains("device_fault_query::automatic(dev.props.vendorID == 0x10DE, dev.checkpoints,"));
        assert!(draw.contains("getenv(\"RX_VK_FAULT_AUTO\")"));
        assert!(draw.contains("fault_last{fault_forced || fault_auto}"));
        let report = function(&draw, "void checkpoints_report() {");
        assert!(report.contains("if (wanted) device_fault_report();"));
    }
}
