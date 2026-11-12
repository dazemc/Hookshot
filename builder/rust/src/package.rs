//! Stage a game folder: the host executable, its libraries, the relocated
//! image, the ROM filesystem and the caches a first start can reuse.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

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
///
/// The link comes first: a tree staged into a new folder has no file there
/// yet, and asking the filesystem whether one is (`to.exists()`) costs a stat
/// per file, 186,000 of them for the romfs. An existing file is kept as it is.
///
/// Folders are shared out over a few threads: the romfs' 186,000 links are
/// mostly time spent waiting for the filesystem, and its top folders differ
/// tenfold in size, so each thread takes the next folder rather than a fixed
/// share.
pub(crate) fn link_tree(src: &Path, dst: &Path, counter: &mut u64) -> Result<(), String> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    // (I/O bound, and one thread per file system is enough: 8 on a 32-core
    // PC, fewer on a small one)
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .clamp(1, 8);
    if threads == 1 {
        let mut linked = 0;
        link_folder(src, dst, &mut linked)?;
        *counter += linked;
        return Ok(());
    }
    let shared = Linker {
        queue: Mutex::new(VecDeque::from([(src.to_path_buf(), dst.to_path_buf())])),
        changed: Condvar::new(),
        busy: AtomicUsize::new(0),
        linked: AtomicU64::new(0),
        error: Mutex::new(None),
    };
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| link_worker(&shared));
        }
    });
    if let Some(error) = shared.error.lock().map_err(|e| e.to_string())?.take() {
        return Err(error);
    }
    *counter += shared.linked.load(Ordering::Relaxed);
    Ok(())
}

/// The folders to stage: a worker takes one, links its files and puts its
/// subfolders back. When none is left and no worker is in a folder, the tree
/// is done.
struct Linker {
    queue: Mutex<VecDeque<(PathBuf, PathBuf)>>,
    changed: Condvar,
    busy: AtomicUsize,
    linked: AtomicU64,
    error: Mutex<Option<String>>,
}

fn link_worker(shared: &Linker) {
    loop {
        if shared.error.lock().is_ok_and(|error| error.is_some()) {
            return;
        }
        let next = {
            let mut queue = match shared.queue.lock() {
                Ok(queue) => queue,
                Err(_) => return,
            };
            loop {
                if let Some(folder) = queue.pop_front() {
                    break Some(folder);
                }
                if shared.busy.load(Ordering::Acquire) == 0 {
                    break None;
                }
                queue = match shared.changed.wait(queue) {
                    Ok(queue) => queue,
                    Err(_) => return,
                };
            }
        };
        let Some((from, to)) = next else {
            return;
        };
        shared.busy.fetch_add(1, Ordering::AcqRel);
        if let Err(error) = link_folder_shared(&from, &to, shared) {
            if let Ok(mut slot) = shared.error.lock()
                && slot.is_none()
            {
                *slot = Some(error);
            }
            if let Ok(mut queue) = shared.queue.lock() {
                queue.clear();
            }
        }
        shared.busy.fetch_sub(1, Ordering::AcqRel);
        shared.changed.notify_all();
    }
}

fn link_folder_shared(from: &Path, to: &Path, shared: &Linker) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut folders = Vec::new();
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            folders.push((source, target));
        } else if kind.is_file() && link_file(&source, &target)? {
            shared.linked.fetch_add(1, Ordering::Relaxed);
        }
    }
    if !folders.is_empty() {
        if let Ok(mut queue) = shared.queue.lock() {
            queue.extend(folders);
        }
        shared.changed.notify_all();
    }
    Ok(())
}

/// One folder at a time, for a single-threaded host and the started tree.
fn link_folder(from: &Path, to: &Path, counter: &mut u64) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from_file = entry.path();
        let to_file = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            link_folder(&from_file, &to_file, counter)?;
        } else if kind.is_file() && link_file(&from_file, &to_file)? {
            *counter += 1;
        }
    }
    Ok(())
}

/// One file of a tree: a hard link, a copy across volumes, and never over a
/// file already staged. True when this call staged it.
fn link_file(from: &Path, to: &Path) -> Result<bool, String> {
    match fs::hard_link(from, to) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(_) => {
            // (no link: another volume, or a filesystem without them. A copy
            // would replace what is there, which the old check kept)
            if to.exists() {
                return Ok(false);
            }
            fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
            Ok(true)
        }
    }
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
) -> Result<Value, String> {
    package_json_on(Platform::current(), work, out, romfs, vk, assets, language)
}

fn package_json_on(
    platform: Platform,
    work: &str,
    out: &str,
    romfs: &str,
    vk: &str,
    assets: &str,
    language: &str,
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

    #[test]
    fn a_staged_tree_links_files_and_keeps_what_is_already_there() {
        let base = temp("link-tree");
        let src = base.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("one.bin"), b"one").unwrap();
        fs::write(src.join("sub/two.bin"), b"two").unwrap();
        let dst = base.join("dst");
        let mut linked = 0;
        link_tree(&src, &dst, &mut linked).unwrap();
        assert_eq!(linked, 2);
        assert_eq!(fs::read(dst.join("one.bin")).unwrap(), b"one");
        assert_eq!(fs::read(dst.join("sub/two.bin")).unwrap(), b"two");
        // (a second pass has nothing to do: the files there are kept)
        let mut again = 0;
        link_tree(&src, &dst, &mut again).unwrap();
        assert_eq!(again, 0);
        // (and a file a player or an earlier build left is not replaced)
        fs::write(dst.join("one.bin"), b"kept").unwrap();
        let mut kept = 0;
        link_tree(&src, &dst, &mut kept).unwrap();
        assert_eq!(kept, 0);
        assert_eq!(fs::read(dst.join("one.bin")).unwrap(), b"kept");
        let _ = fs::remove_dir_all(base);
    }
}
