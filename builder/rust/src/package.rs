//! Stage a game folder: the host executable, its libraries, the relocated
//! image, the ROM filesystem and the caches a first start can reuse.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::platform::Platform;

const LANGUAGES: [(&str, &str); 16] = [
    ("en-US", "English (US)"),
    ("en-GB", "English (UK)"),
    ("fr", "French (France)"),
    ("fr-CA", "French (Canada)"),
    ("de", "German"),
    ("it", "Italian"),
    ("es", "Spanish (Spain)"),
    ("es-419", "Spanish (Latin America)"),
    ("pt-BR", "Portuguese (Brazil)"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("ru", "Russian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh-Hans", "Chinese (Simplified)"),
    ("zh-Hant", "Chinese (Traditional)"),
];

fn link_or_copy(src: &Path, dst: &Path) -> Result<u64, String> {
    if !src.is_file() {
        return Ok(0);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if dst.exists() {
        return Ok(0);
    }
    let size = fs::metadata(src).map(|meta| meta.len()).unwrap_or(0);
    if fs::hard_link(src, dst).is_err() {
        fs::copy(src, dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    }
    Ok(size)
}

fn copy(src: &Path, dst: &Path) -> Result<u64, String> {
    if !src.is_file() {
        return Ok(0);
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let size = fs::metadata(src).map(|meta| meta.len()).unwrap_or(0);
    fs::copy(src, dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(size)
}

/// Hard links where the disk allows (the 26 GB romfs is not written twice),
/// copies across volumes.
pub(crate) fn link_tree(src: &Path, dst: &Path, counter: &mut u64) -> Result<(), String> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            link_tree(&from, &to, counter)?;
        } else if kind.is_file() && !to.exists() {
            if fs::hard_link(&from, &to).is_err() {
                fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
            }
            *counter += 1;
        }
    }
    Ok(())
}

struct LibrarySource {
    name: &'static str,
    source: PathBuf,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The runtime libraries staged beside the executable in `bin`. Linux keeps
/// the AppImage's single SDL2; Windows copies the DLLs nx2/make_game.py's
/// `BESIDE` list names, the SDL ones from third_party/sdl2 and wgpu-native
/// from WGPU_NATIVE.
fn runtime_libraries(
    platform: Platform,
    root: &Path,
    assets: Option<&Path>,
    sdl2: Option<&Path>,
    wgpu: Option<&Path>,
) -> Vec<LibrarySource> {
    match platform {
        Platform::Linux => {
            let source = sdl2
                .filter(|path| path.is_file())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.join("build-linux/deps/sdl2/libSDL2-2.0.so.0"));
            vec![LibrarySource {
                name: platform.primary_sdl_library(),
                source,
            }]
        }
        Platform::Windows => {
            let mut libraries = Vec::new();
            for name in ["SDL2.dll", "SDL3.dll"] {
                let built = root.join("third_party/sdl2").join(name);
                let source = if built.is_file() {
                    built
                } else {
                    assets.map(|assets| assets.join(name)).unwrap_or(built)
                };
                libraries.push(LibrarySource { name, source });
            }
            let built = wgpu
                .map(|path| {
                    if path.is_dir() {
                        path.join("lib/wgpu_native.dll")
                    } else {
                        path.to_path_buf()
                    }
                })
                .unwrap_or_else(|| root.join("third_party/wgpu-native/lib/wgpu_native.dll"));
            let source = if built.is_file() {
                built
            } else {
                assets
                    .map(|assets| assets.join("wgpu_native.dll"))
                    .unwrap_or(built)
            };
            libraries.push(LibrarySource {
                name: "wgpu_native.dll",
                source,
            });
            libraries
        }
    }
}

/// The Vulkan back end: an explicit path, else the standalone build's own,
/// else the assets folder's copy.
fn vulkan_library_source(
    platform: Platform,
    root: &Path,
    work: &Path,
    vk: &str,
    assets: Option<&Path>,
) -> PathBuf {
    if !vk.is_empty() {
        return PathBuf::from(vk);
    }
    let built = platform.built_vulkan(root, work);
    if built.is_file() {
        return built;
    }
    assets
        .map(|assets| assets.join(platform.vulkan_library()))
        .unwrap_or_default()
}

pub fn package_json(
    work: &str,
    out: &str,
    romfs: &str,
    vk: &str,
    assets: &str,
    language: &str,
    ngx: &str,
) -> Result<Value, String> {
    package_json_on(
        Platform::current(),
        work,
        out,
        romfs,
        vk,
        assets,
        language,
        ngx,
    )
}

/// DLSS's runtime in `<out>/ngx`, as Python's `place_ngx_runtime` does: the
/// SDK's file when a DLSS build names the folder, and out of the way when it
/// does not (a folder rebuilt without DLSS loses the runtime too).
fn place_ngx_runtime(platform: Platform, out: &Path, sdk: &Path) -> Value {
    let ngx = match platform {
        Platform::Linux => crate::ngx_sdk::Platform::Linux,
        Platform::Windows => crate::ngx_sdk::Platform::Windows,
    };
    if !sdk.as_os_str().is_empty() {
        return crate::ngx_sdk::install_runtime(sdk, ngx, &out.join("ngx"))
            .unwrap_or_else(|error| json!({"placed": false, "error": error}));
    }
    let stale = out.join("ngx").join(ngx.runtime_name());
    let removed = stale.is_file() && fs::remove_file(&stale).is_ok();
    if removed {
        let _ = fs::remove_dir(out.join("ngx"));
    }
    json!({"placed": false, "removed": removed})
}

#[allow(clippy::too_many_arguments)]
fn package_json_on(
    platform: Platform,
    work: &str,
    out: &str,
    romfs: &str,
    vk: &str,
    assets: &str,
    language: &str,
    ngx: &str,
) -> Result<Value, String> {
    let root = crate::kit::root();
    let work = Path::new(work);
    let out = Path::new(out);
    let assets = Path::new(assets);
    let assets_dir = (!assets.as_os_str().is_empty()).then_some(assets);
    let bin = out.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let exe = bin.join(platform.executable());
    if !exe.is_file() {
        return Err(format!("{} is not built yet", exe.display()));
    }
    let mut bytes = 0u64;
    let mut linked = 0u64;
    bytes += copy(&work.join("sa/image/image.bin"), &out.join("image.bin"))?;
    let vk = vulkan_library_source(platform, &root, work, vk, assets_dir);
    if vk.is_file() {
        bytes += copy(&vk, &bin.join(platform.vulkan_library()))?;
    }
    // (NVIDIA DLSS: DLSS's runtime in <out>/ngx, never NGX's core)
    let ngx_runtime = place_ngx_runtime(platform, out, Path::new(ngx));
    let sdl2 = env_path("HOOKSHOT_SDL2");
    let wgpu = env_path("WGPU_NATIVE");
    for library in runtime_libraries(
        platform,
        &root,
        assets_dir,
        sdl2.as_deref(),
        wgpu.as_deref(),
    ) {
        if library.source.is_file() {
            bytes += copy(&library.source, &bin.join(library.name))?;
        }
    }
    bytes += copy(&root.join("assets/hookshot.png"), &out.join("hookshot.png"))?;
    let romfs = Path::new(romfs);
    let target_romfs = out.join("romfs");
    if romfs.is_dir() && !target_romfs.exists() {
        link_tree(romfs, &target_romfs, &mut linked)?;
    }
    if let Some(assets) = assets_dir {
        for name in [
            "hookshot_shaders.bin",
            "hookshot_pipeline_states_shipped.bin",
            "hookshot_prepared.bin",
            "mkw_vk_pipelines.bin",
            "hookshot_pipelines.bin",
            "hookshot_computes.bin",
            "prompts/",
        ] {
            let mut count = 0;
            if name.ends_with('/') {
                link_tree(
                    &assets.join(name.trim_end_matches('/')),
                    &out.join(name.trim_end_matches('/')),
                    &mut count,
                )?;
                linked += count;
            } else {
                bytes += link_or_copy(&assets.join(name), &out.join(name))?;
            }
        }
    }
    let seed = crate::stage::seed_pipelines_json(
        root.to_str().unwrap_or_default(),
        out.to_str().unwrap_or_default(),
    )?;
    if !seed["copied"].as_bool().unwrap_or(false) {
        // (no seed in a development checkout: keep the one the assets brought)
    }
    let language = if LANGUAGES.iter().any(|(code, _)| *code == language) {
        language
    } else {
        "en-US"
    };
    let mut text = String::from(
        "# The game's language. Change the line below to one of the codes listed here, save, and start the game again.\n\n",
    );
    text.push_str(language);
    text.push_str("\n\n");
    for (code, name) in LANGUAGES {
        text.push_str(&format!("# {code:<8} {name}\n"));
    }
    fs::write(out.join("language.txt"), text).map_err(|e| e.to_string())?;
    let release = crate::version::release(&root);
    let mut version = String::from("profiles=1\n");
    if !release.is_empty() && release != "unknown" {
        version.push_str(&format!("version={release}\n"));
    }
    fs::write(out.join("hookshot_version.txt"), version).map_err(|e| e.to_string())?;
    // The button prompts of other controllers, as Python's make_game makes
    // them: never fatal, and a failure leaves the game its own icons.
    let prompts = if romfs.is_dir() {
        match crate::prompts::build(romfs, &out.join("prompts")) {
            Ok(written) => json!({
                "font_bytes": written.font_bytes,
                "size_table": written.size_table,
                "faces": written.faces,
                "hint": written.hint,
                "ring": written.ring,
            }),
            Err(error) => json!({"error": error}),
        }
    } else {
        json!({"skipped": true})
    };
    Ok(json!({
        "stage": "packaged",
        "exe": exe.to_string_lossy(),
        "image": out.join("image.bin").to_string_lossy(),
        "romfs": target_romfs.to_string_lossy(),
        "bytes": bytes,
        "linked": linked,
        "language": language,
        "vk": bin.join(platform.vulkan_library()).is_file(),
        "sdl": bin.join(platform.primary_sdl_library()).is_file(),
        "ngx": ngx_runtime,
        "seed": seed,
        "prompts": prompts,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "hookshot-rust-package-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn linux_libraries_keep_the_old_names_and_paths() {
        let base = temp("linux-libs");
        let root = base.join("root");
        let libraries = runtime_libraries(Platform::Linux, &root, None, None, None);
        assert_eq!(libraries.len(), 1);
        assert_eq!(libraries[0].name, "libSDL2-2.0.so.0");
        assert_eq!(
            libraries[0].source,
            root.join("build-linux/deps/sdl2/libSDL2-2.0.so.0")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn linux_sdl_override_wins_when_it_exists() {
        let base = temp("linux-sdl");
        let root = base.join("root");
        let sdl = base.join("sdl/libSDL2-2.0.so.0");
        fs::create_dir_all(sdl.parent().unwrap()).unwrap();
        fs::write(&sdl, b"SDL").unwrap();
        let libraries = runtime_libraries(Platform::Linux, &root, None, Some(&sdl), None);
        assert_eq!(libraries[0].source, sdl);
        let missing = base.join("missing/libSDL2-2.0.so.0");
        let libraries = runtime_libraries(Platform::Linux, &root, None, Some(&missing), None);
        assert_eq!(
            libraries[0].source,
            root.join("build-linux/deps/sdl2/libSDL2-2.0.so.0")
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_libraries_use_the_builder_names_and_sources() {
        let base = temp("windows-libs");
        let root = base.join("root");
        let sdl = root.join("third_party/sdl2");
        fs::create_dir_all(&sdl).unwrap();
        fs::write(sdl.join("SDL2.dll"), b"SDL2").unwrap();
        fs::write(sdl.join("SDL3.dll"), b"SDL3").unwrap();
        let wgpu = base.join("wgpu-native");
        fs::create_dir_all(wgpu.join("lib")).unwrap();
        fs::write(wgpu.join("lib/wgpu_native.dll"), b"wgpu").unwrap();
        let libraries = runtime_libraries(Platform::Windows, &root, None, None, Some(&wgpu));
        assert_eq!(
            libraries
                .iter()
                .map(|library| library.name)
                .collect::<Vec<_>>(),
            ["SDL2.dll", "SDL3.dll", "wgpu_native.dll"]
        );
        assert_eq!(libraries[0].source, sdl.join("SDL2.dll"));
        assert_eq!(libraries[1].source, sdl.join("SDL3.dll"));
        assert_eq!(libraries[2].source, wgpu.join("lib/wgpu_native.dll"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_libraries_fall_back_to_the_assets_folder() {
        let base = temp("windows-assets");
        let root = base.join("root");
        let assets = base.join("assets");
        fs::create_dir_all(&assets).unwrap();
        for name in ["SDL2.dll", "SDL3.dll", "wgpu_native.dll"] {
            fs::write(assets.join(name), b"dll").unwrap();
        }
        let libraries = runtime_libraries(Platform::Windows, &root, Some(&assets), None, None);
        assert_eq!(libraries[0].source, assets.join("SDL2.dll"));
        assert_eq!(libraries[1].source, assets.join("SDL3.dll"));
        assert_eq!(libraries[2].source, assets.join("wgpu_native.dll"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn vulkan_library_prefers_explicit_then_built_then_assets() {
        let base = temp("vulkan-source");
        let root = base.join("root");
        let work = base.join("work");
        let assets = base.join("assets");
        fs::create_dir_all(&assets).unwrap();
        let built = work.join("sa/build/hookshot_vk.dll");
        fs::create_dir_all(built.parent().unwrap()).unwrap();
        fs::write(&built, b"vk").unwrap();
        let explicit = base.join("given/hookshot_vk.dll");
        assert_eq!(
            vulkan_library_source(
                Platform::Windows,
                &root,
                &work,
                explicit.to_str().unwrap(),
                Some(&assets)
            ),
            explicit
        );
        assert_eq!(
            vulkan_library_source(Platform::Windows, &root, &work, "", Some(&assets)),
            built
        );
        fs::remove_file(&built).unwrap();
        let from_assets = assets.join("hookshot_vk.dll");
        fs::write(&from_assets, b"vk").unwrap();
        assert_eq!(
            vulkan_library_source(Platform::Windows, &root, &work, "", Some(&assets)),
            from_assets
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_linux_package_takes_the_built_vulkan_backend_from_the_work_folder() {
        let base = temp("linux-built-vulkan");
        let work = base.join("work");
        let out = base.join("out");
        fs::create_dir_all(work.join("sa/image")).unwrap();
        fs::write(work.join("sa/image/image.bin"), b"image").unwrap();
        fs::create_dir_all(work.join("sa/build")).unwrap();
        fs::write(work.join("sa/build/hookshot_vk.so"), b"vk").unwrap();
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/hookshot"), b"exe").unwrap();
        let answer = package_json_on(
            Platform::Linux,
            work.to_str().unwrap(),
            out.to_str().unwrap(),
            "",
            "",
            "",
            "en-US",
            "",
        )
        .unwrap();
        assert_eq!(answer["vk"], true);
        assert_eq!(
            fs::read(out.join("bin/hookshot_vk.so")).unwrap(),
            b"vk".to_vec()
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn linux_package_keeps_the_appimage_layout() {
        let base = temp("linux-package");
        let work = base.join("work");
        let out = base.join("out");
        fs::create_dir_all(work.join("sa/image")).unwrap();
        fs::write(work.join("sa/image/image.bin"), b"image").unwrap();
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/hookshot"), b"exe").unwrap();
        let vk = base.join("hookshot_vk.so");
        fs::write(&vk, b"vk").unwrap();
        let assets = base.join("assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("hookshot_shaders.bin"), b"shaders").unwrap();
        let answer = package_json_on(
            Platform::Linux,
            work.to_str().unwrap(),
            out.to_str().unwrap(),
            "",
            vk.to_str().unwrap(),
            assets.to_str().unwrap(),
            "en-US",
            "",
        )
        .unwrap();
        assert_eq!(answer["stage"], "packaged");
        assert_eq!(
            Path::new(answer["exe"].as_str().unwrap()),
            out.join("bin/hookshot")
        );
        assert_eq!(answer["vk"], true);
        assert_eq!(answer["seed"]["copied"], false);
        assert!(out.join("bin/hookshot_vk.so").is_file());
        assert!(!out.join("bin/hookshot.exe").exists());
        assert!(out.join("image.bin").is_file());
        assert!(out.join("hookshot_shaders.bin").is_file());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn windows_package_puts_the_executable_and_dlls_in_bin() {
        let base = temp("windows-package");
        let work = base.join("work");
        let out = base.join("out");
        fs::create_dir_all(work.join("sa/image")).unwrap();
        fs::write(work.join("sa/image/image.bin"), b"image").unwrap();
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/hookshot.exe"), b"exe").unwrap();
        let vk = base.join("hookshot_vk.dll");
        fs::write(&vk, b"vk").unwrap();
        let assets = base.join("assets");
        fs::create_dir_all(&assets).unwrap();
        for name in ["SDL2.dll", "SDL3.dll", "wgpu_native.dll"] {
            fs::write(assets.join(name), b"dll").unwrap();
        }
        let answer = package_json_on(
            Platform::Windows,
            work.to_str().unwrap(),
            out.to_str().unwrap(),
            "",
            vk.to_str().unwrap(),
            assets.to_str().unwrap(),
            "en-US",
            "",
        )
        .unwrap();
        assert_eq!(answer["stage"], "packaged");
        assert_eq!(answer["vk"], true);
        assert_eq!(answer["sdl"], true);
        for name in [
            "hookshot.exe",
            "hookshot_vk.dll",
            "SDL2.dll",
            "SDL3.dll",
            "wgpu_native.dll",
        ] {
            assert!(
                out.join("bin").join(name).is_file(),
                "{name} was not staged"
            );
        }
        assert!(out.join("image.bin").is_file());
        assert!(!out.join("bin").join("hookshot").exists());
        let _ = fs::remove_dir_all(base);
    }

    fn package_into(base: &Path, platform: Platform, ngx: &str) -> Value {
        let work = base.join("work");
        let out = base.join("out");
        fs::create_dir_all(work.join("sa/image")).unwrap();
        fs::write(work.join("sa/image/image.bin"), b"image").unwrap();
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin").join(platform.executable()), b"exe").unwrap();
        package_json_on(
            platform,
            work.to_str().unwrap(),
            out.to_str().unwrap(),
            "",
            "",
            "",
            "en-US",
            ngx,
        )
        .unwrap()
    }

    #[test]
    fn a_dlss_package_puts_the_runtime_in_ngx_and_never_its_core() {
        let base = temp("ngx-runtime");
        let platform = crate::ngx_sdk::Platform::Linux;
        let sdk = base.join("sdk");
        let runtime = crate::ngx_sdk::runtime(&sdk, platform);
        fs::create_dir_all(runtime.parent().unwrap()).unwrap();
        fs::write(&runtime, b"runtime").unwrap();
        let answer = package_into(&base, Platform::Linux, sdk.to_str().unwrap());
        assert_eq!(answer["ngx"]["placed"], true);
        assert!(base.join("out/ngx").join(platform.runtime_name()).is_file());
        assert!(!base.join("out/ngx/_nvngx.so").exists());
        // (a folder rebuilt without DLSS loses the runtime, as Python's does)
        let answer = package_into(&base, Platform::Linux, "");
        assert_eq!(answer["ngx"]["placed"], false);
        assert_eq!(answer["ngx"]["removed"], true);
        assert!(!base.join("out/ngx").exists());
        let _ = fs::remove_dir_all(base);
    }
}
