// The Graphics > Detail > Texture detail row (gpu/vk/vk_texture.cpp, vk_menu.cpp, vk_menu.h; upstream NotRexed/Hookshot
// 3aae87ea, #2557). Game-free: the real mapping function is cut out and compiled, and the menu and texture source are read.
// No game, GPU or Vulkan headers are needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A C++ definition's braces, matched, from its first line.
fn definition(text: &str, marker: &str) -> String {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + text[start..].find('{').expect("opening brace") + 1;
    let bytes = text.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    text[start..end].to_string()
}

fn cxx() -> Option<String> {
    let candidates =
        std::env::var("CXX")
            .ok()
            .into_iter()
            .chain(["c++".into(), "g++".into(), "clang++".into()]);
    for candidate in candidates {
        if Command::new(&candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

#[test]
fn texture_detail_maps_auto_full_medium_and_low() {
    let texture = source("gpu/vk/vk_texture.cpp");
    let dims = definition(
        &texture,
        "DetailDims texture_detail_dims(int detail, bool low_vram)",
    );
    let Some(cxx) = cxx() else {
        eprintln!(
            "skipping the compiled texture detail mapping: no C++ compiler (CXX, c++, g++ or clang++)"
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-texture-detail-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let file = dir.join("detail.cpp");
    let exe = dir.join(if cfg!(windows) {
        "detail.exe"
    } else {
        "detail"
    });
    let fixture = format!(
        r#"#include <cstdint>
#include <cstdio>
struct DetailDims {{ uint32_t max_dim; uint64_t skip_from_mb; }};
{dims}
#define CHECK(x) do {{ if (!(x)) {{ std::fprintf(stderr, "FAIL %d: %s\n", __LINE__, #x); return 1; }} }} while (0)
int main() {{
    DetailDims a = texture_detail_dims(0, true);    // (Auto on a card of 4.5 GB or less at Minimum: Medium)
    CHECK(a.max_dim == 1024 && a.skip_from_mb == 8);
    DetailDims b = texture_detail_dims(0, false);   // (Auto on a larger card: the game's own)
    CHECK(b.max_dim == 2048 && b.skip_from_mb == 64);
    DetailDims c = texture_detail_dims(1, true);    // (Full: the game's own, whatever the card)
    CHECK(c.max_dim == 2048 && c.skip_from_mb == 64);
    DetailDims d = texture_detail_dims(2, false);   // (Medium)
    CHECK(d.max_dim == 1024 && d.skip_from_mb == 8);
    DetailDims e = texture_detail_dims(3, false);   // (Low)
    CHECK(e.max_dim == 512 && e.skip_from_mb == 8);
    puts("ok");
    return 0;
}}
"#
    );
    fs::write(&file, fixture).expect("write fixture");
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C++ compiler");
    assert!(build.success(), "{cxx} failed on {}", file.display());
    let run = Command::new(&exe).output().expect("run the fixture");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
    let _ = fs::remove_dir_all(dir);

    let menu = source("gpu/vk/vk_menu.cpp");
    let header = source("gpu/vk/vk_menu.h");
    assert!(header.contains("int texture_detail = 0;"));
    assert!(menu.contains("\"Texture detail\", &opts.texture_detail, 0, 3, 1, tx_texture_detail"));
    assert!(menu.contains("static const char* t[] = {\"Auto\", \"Full\", \"Medium\", \"Low\"};"));
    assert!(
        texture.contains("const DetailDims dims = texture_detail_dims(opts.texture_detail, low_vram_texture_mips());"),
        "the row is not read as each texture is made"
    );
    assert!(
        texture.contains("const char* dim_env = getenv(\"RX_VK_TEX_MAX_DIM\");")
            && texture.contains("const char* skip_env = getenv(\"RX_VK_TEX_SKIP_MB\");"),
        "the environment overrides no longer win"
    );
    assert!(
        texture.contains("bool low_vram_texture_mips()"),
        "Auto no longer keeps the low-VRAM rule"
    );
}
