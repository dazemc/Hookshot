//! Stage a game folder: the host executable, its libraries, the relocated
//! image, the ROM filesystem and the caches a first start can reuse.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

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
fn link_tree(src: &Path, dst: &Path, counter: &mut u64) -> Result<(), String> {
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

pub fn package_json(
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
    let bin = out.join("bin");
    fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let exe = bin.join("hookshot");
    if !exe.is_file() {
        return Err(format!("{} is not built yet", exe.display()));
    }
    let mut bytes = 0u64;
    let mut linked = 0u64;
    bytes += copy(&work.join("sa/image/image.bin"), &out.join("image.bin"))?;
    let vk = if vk.is_empty() {
        let built = root.join("build-linux/vk-work/sa/build/hookshot_vk.so");
        if built.is_file() {
            built
        } else if !assets.is_empty() {
            PathBuf::from(assets).join("hookshot_vk.so")
        } else {
            PathBuf::new()
        }
    } else {
        PathBuf::from(vk)
    };
    if vk.is_file() {
        bytes += copy(&vk, &bin.join("hookshot_vk.so"))?;
    }
    let sdl = Path::new(&std::env::var("HOOKSHOT_SDL2").unwrap_or_default()).to_path_buf();
    let sdl = if sdl.is_file() {
        sdl
    } else {
        root.join("build-linux/deps/sdl2/libSDL2-2.0.so.0")
    };
    if sdl.is_file() {
        bytes += copy(&sdl, &bin.join("libSDL2-2.0.so.0"))?;
    }
    bytes += copy(&root.join("assets/hookshot.png"), &out.join("hookshot.png"))?;
    let romfs = Path::new(romfs);
    let target_romfs = out.join("romfs");
    if romfs.is_dir() && !target_romfs.exists() {
        link_tree(romfs, &target_romfs, &mut linked)?;
    }
    if !assets.is_empty() {
        let assets = Path::new(assets);
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
    Ok(json!({
        "stage": "packaged",
        "exe": exe.to_string_lossy(),
        "image": out.join("image.bin").to_string_lossy(),
        "romfs": target_romfs.to_string_lossy(),
        "bytes": bytes,
        "linked": linked,
        "language": language,
        "vk": bin.join("hookshot_vk.so").is_file(),
        "sdl": bin.join("libSDL2-2.0.so.0").is_file(),
        "seed": seed,
    }))
}
