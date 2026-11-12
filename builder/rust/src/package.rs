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

/// The language a game folder was made in (`language.txt`'s code line), if
/// it still has one the game knows.
pub fn folder_language(out: &Path) -> Option<String> {
    let text = fs::read_to_string(out.join("language.txt")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((code, _)) = LANGUAGES.iter().find(|(code, _)| *code == line) {
            return Some((*code).to_owned());
        }
    }
    None
}

/// The game language closest to a system name ("de-DE" or "pt_BR.UTF-8"), as
/// the Windows builder's window picks its first one: the exact one, then the
/// same family (`es-MX` is Spanish (Latin America), `zh-TW` Chinese
/// (Traditional)). `None` when the game has none of it.
pub fn language_for_system_name(name: &str) -> Option<String> {
    let name = name
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('_', "-");
    let family = name.split('-').next().unwrap_or("").to_lowercase();
    if let Some((code, _)) = LANGUAGES
        .iter()
        .find(|(code, _)| code.eq_ignore_ascii_case(&name))
    {
        return Some((*code).to_owned());
    }
    if family == "zh" {
        return Some(
            if ["TW", "HK", "MO", "Hant"]
                .iter()
                .any(|tag| name.contains(tag))
            {
                "zh-Hant"
            } else {
                "zh-Hans"
            }
            .to_owned(),
        );
    }
    if family == "es" && !name.eq_ignore_ascii_case("es") && !name.eq_ignore_ascii_case("es-es") {
        return Some("es-419".to_owned());
    }
    if family == "pt" {
        return Some("pt-BR".to_owned());
    }
    LANGUAGES
        .iter()
        .find(|(code, _)| *code == family)
        .map(|(code, _)| (*code).to_owned())
}

/// The system's own locale name, as gettext reads it: the first of `LC_ALL`,
/// `LC_MESSAGES` and `LANG` that is set, and `LANGUAGE`'s first entry over it
/// unless that is C. (The names come in so the order is testable.)
pub fn system_name_from_env(
    language: &str,
    lc_all: &str,
    lc_messages: &str,
    lang: &str,
) -> String {
    let mut name = [lc_all, lc_messages, lang]
        .into_iter()
        .find(|value| !value.is_empty())
        .unwrap_or("");
    let first = language.split(':').next().unwrap_or("");
    let base = name.split(['.', '@']).next().unwrap_or("");
    if !first.is_empty() && !name.is_empty() && base != "C" && base != "POSIX" {
        name = first;
    }
    name.to_owned()
}

/// Windows' display language ("de-DE"): the first of the user's preferred UI
/// languages, else the user's default UI language. Not the regional format,
/// which is often another.
#[cfg(windows)]
fn windows_ui_language() -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    unsafe extern "system" {
        fn GetUserPreferredUILanguages(
            flags: u32,
            count: *mut u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn GetUserDefaultUILanguage() -> u16;
        fn LCIDToLocaleName(lcid: u32, name: *mut u16, cch: i32, flags: u32) -> i32;
    }
    const MUI_LANGUAGE_NAME: u32 = 0x8;
    let mut count = 0u32;
    let mut size = 0u32;
    let ok = unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, std::ptr::null_mut(), &mut size)
    };
    if ok != 0 && size > 0 {
        let mut buffer = vec![0u16; size as usize];
        let ok = unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                buffer.as_mut_ptr(),
                &mut size,
            )
        };
        if ok != 0 {
            let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(0);
            if end > 0 {
                return Some(
                    std::ffi::OsString::from_wide(&buffer[..end])
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut buffer = [0u16; 85];
    let lcid = unsafe { GetUserDefaultUILanguage() } as u32;
    let len = unsafe { LCIDToLocaleName(lcid, buffer.as_mut_ptr(), buffer.len() as i32, 0) };
    (len > 1).then(|| {
        std::ffi::OsString::from_wide(&buffer[..len as usize - 1])
            .to_string_lossy()
            .into_owned()
    })
}

/// The game language closest to the system's own, or `None` when the game has
/// none of it.
pub fn system_language() -> Option<String> {
    #[cfg(windows)]
    {
        language_for_system_name(&windows_ui_language().unwrap_or_default())
    }
    #[cfg(not(windows))]
    {
        let name = system_name_from_env(
            &std::env::var("LANGUAGE").unwrap_or_default(),
            &std::env::var("LC_ALL").unwrap_or_default(),
            &std::env::var("LC_MESSAGES").unwrap_or_default(),
            &std::env::var("LANG").unwrap_or_default(),
        );
        language_for_system_name(&name)
    }
}

/// The language a build writes to the folder's `language.txt`: the one asked
/// for; else the one the folder has, from the build before or the player's own
/// change to the file; else, in a launcher's build (`prepare`: no window to
/// choose one in), the system's own; else English (US).
pub fn build_language(out: &Path, requested: &str, prepare: bool) -> String {
    if !requested.is_empty() {
        return requested.to_owned();
    }
    if let Some(kept) = folder_language(out) {
        return kept;
    }
    if prepare {
        if let Some(system) = system_language() {
            return system;
        }
    }
    "en-US".to_owned()
}

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
    // (a build with no language keeps the folder's, or takes the system's in
    // a launcher's first one: make_game's rule)
    let language = build_language(out, language, false);
    let language = if LANGUAGES.iter().any(|(code, _)| *code == language) {
        language
    } else {
        "en-US".to_owned()
    };
    let mut text = String::from(
        "# The game's language. Change the line below to one of the codes listed here, save, and start the game again.\n\n",
    );
    text.push_str(&language);
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
                "kept": written.kept,
                "reason": written.reason,
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
    fn a_folder_keeps_the_language_it_was_built_in() {
        let base = temp("language");
        let out = base.join("out");
        fs::create_dir_all(&out).unwrap();
        assert_eq!(build_language(&out, "", false), "en-US");
        fs::write(
            out.join("language.txt"),
            "# The game's language. Change the line below.\n\nru\n\n# en-US English (US)\n",
        )
        .unwrap();
        assert_eq!(folder_language(&out).as_deref(), Some("ru"));
        assert_eq!(build_language(&out, "", false), "ru");
        assert_eq!(build_language(&out, "", true), "ru");
        // (a language asked for is still the one built)
        assert_eq!(build_language(&out, "de", true), "de");
        // (a code the game does not know is no language of the folder)
        fs::write(out.join("language.txt"), "xx-XX\n").unwrap();
        assert_eq!(folder_language(&out), None);
        assert_eq!(build_language(&out, "", false), "en-US");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_system_name_takes_its_family() {
        for (name, want) in [
            ("de_DE.UTF-8", Some("de")),
            ("pt_PT", Some("pt-BR")),
            ("pt_BR.UTF-8", Some("pt-BR")),
            ("es_MX", Some("es-419")),
            ("es-ES", Some("es")),
            ("es", Some("es")),
            ("zh_TW", Some("zh-Hant")),
            ("zh-Hant-HK", Some("zh-Hant")),
            ("zh_CN", Some("zh-Hans")),
            ("en_US", Some("en-US")),
            ("fr_CA.UTF-8", Some("fr-CA")),
            ("C", None),
        ] {
            assert_eq!(language_for_system_name(name).as_deref(), want, "{name}");
        }
    }

    #[test]
    fn the_locale_environment_is_read_as_gettext_does() {
        assert_eq!(system_name_from_env("", "de_DE.UTF-8", "", "en_US"), "de_DE.UTF-8");
        assert_eq!(system_name_from_env("", "", "fr_FR", "en_US"), "fr_FR");
        assert_eq!(system_name_from_env("", "", "", "en_US"), "en_US");
        // (LANGUAGE's first entry over the locale, unless that is C)
        assert_eq!(system_name_from_env("pt_BR:fr_FR", "en_US.UTF-8", "", "C"), "pt_BR");
        assert_eq!(system_name_from_env("pt_BR:fr_FR", "C.UTF-8", "", "C"), "C.UTF-8");
        assert_eq!(system_name_from_env("pt_BR", "", "", ""), "");
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
    fn a_package_with_no_language_keeps_the_folders() {
        let base = temp("linux-language");
        let work = base.join("work");
        let out = base.join("out");
        fs::create_dir_all(work.join("sa/image")).unwrap();
        fs::write(work.join("sa/image/image.bin"), b"image").unwrap();
        fs::create_dir_all(out.join("bin")).unwrap();
        fs::write(out.join("bin/hookshot"), b"exe").unwrap();
        fs::write(out.join("language.txt"), "# the game's language\n\nru\n").unwrap();
        let answer = package_json_on(
            Platform::Linux,
            work.to_str().unwrap(),
            out.to_str().unwrap(),
            "",
            "",
            "",
            "",
        )
        .unwrap();
        assert_eq!(answer["language"], "ru");
        let text = fs::read_to_string(out.join("language.txt")).unwrap();
        assert!(text.lines().any(|line| line == "ru"), "{text}");
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
}
