// Gameplay > Skip opening (host/hos_main.c hos_oot_opening, nx2/oot_patches.py, vk_menu.cpp; upstream NotRexed/Hookshot
// 435d83d3, #2827). Game-free: the host function is cut out and compiled with stubs, and the menu and patch sources are
// read. Only the new-game story movie and Navi waking Link are skipped; Meet_DekuTree, later cutscenes and Skip title
// (option 44) stay. The row is saved like the other on/off rows, off by default.

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

fn function(text: &str, marker: &str) -> String {
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

fn cc() -> Option<String> {
    let candidates =
        std::env::var("CC")
            .ok()
            .into_iter()
            .chain(["cc".into(), "gcc".into(), "clang".into()]);
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
fn skip_opening_is_wired_and_persisted_off_by_default() {
    let api = source("gpu/vk/vk_api.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let header = source("gpu/vk/vk_menu.h");
    let replay = source("gpu/vk/vk_replay.cpp");
    let host = source("host/hos_main.c");
    let patches = source("nx2/oot_patches.py");
    let rust_patches = source("builder/rust/src/oot_patches.rs");

    assert!(api.contains("enum { HOS_OPT_SKIP_OPENING = 52 };"));
    assert!(api.contains("44 stays reserved for Skip title"));
    assert!(
        header.contains("int skip_opening = 0;"),
        "Skip opening does not default off"
    );
    assert!(menu.contains("\"Skip opening\", &opts.skip_opening, 0, 1, 1, tx_onoff"));
    assert!(
        menu.contains("if (const char* e = getenv(\"HOS_SKIP_OPENING\")) env_override(&opts.skip_opening, !env_value_off(e));"),
        "a run cannot turn it on without saving it"
    );
    assert!(replay.contains("if (id == HOS_OPT_SKIP_OPENING) return opts.skip_opening;"));
    assert!(
        menu.contains("const char* ini_key(const Row& r)")
            && menu.contains("fprintf(f, \"%s=%d\\n\", ini_key(r), saved_value(r.value));"),
        "the row is not saved by the other on/off rows' writer"
    );

    let opening = function(&host, "int hos_oot_opening(Ctx *c, int mode) {");
    assert!(opening.contains("strcmp(name, \"Opening_Introduction\")"));
    assert!(opening.contains("strcmp(name, \"Opening_LoadingKokiriForest\")"));
    assert_eq!(opening.matches("strcmp(name, \"Opening_").count(), 2);
    assert!(
        opening.contains("strcmp(name, \"Dm_Kokiri_0000\")")
            && opening.contains("strcmp(name, \"Dm_Kokiri_0030\")")
    );
    assert_eq!(opening.matches("strcmp(name, \"Dm_Kokiri_").count(), 2);
    assert!(opening.contains("Meet_DekuTree") && opening.contains("is not skipped"));
    assert!(opening.contains("return 0;   /* the request is still sent */"));
    assert!(opening.contains("rx_option(HOS_OPT_SKIP_OPENING, 0)"));
    assert!(!opening.contains("HOS_OPT_SKIP_TITLE") && !opening.contains("0x2f8"));
    assert!(host.contains("if (!oot_event_playing(c)) hos_oot_opening(c, 3);"));

    for hook in [
        "OPENING_REQUEST_HEAD = \"void f_80d9a820",
        "OPENING_TIMELINE_HEAD = \"void f_83b5bfb0",
        "OPENING_QUERY_HEAD = \"void f_80de6cb0",
        "def patch_opening(source):",
        "opening = patch_opening(out) if protect_hearts else None",
    ] {
        assert!(patches.contains(hook), "nx2/oot_patches.py misses {hook}");
    }
    assert!(patches.contains("done[\"opening skip\"] != 3"));
    for part in [
        "OPENING_REQUEST_HEAD",
        "OPENING_TIMELINE_HEAD",
        "OPENING_QUERY_HEAD",
        "fn patch_opening(s: &str)",
        "patch_opening(&out)",
        "the opening skip was found at {} of 3 functions",
    ] {
        assert!(
            rust_patches.contains(part),
            "the Rust engine's patches miss {part}"
        );
    }

    let Some(cc) = cc() else {
        eprintln!(
            "skipping the compiled skip-opening function: no C compiler (CC, cc, gcc or clang)"
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-skip-opening-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let file = dir.join("opening.c");
    let exe = dir.join(if cfg!(windows) {
        "opening.exe"
    } else {
        "opening"
    });
    let fixture = format!(
        r#"#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef struct {{ uint64_t x[32]; }} Ctx;
static uint8_t mem[0x4000];
static char said[2048];
static int opt;
enum {{ HOS_OPT_SKIP_OPENING = 52 }};
static uint64_t recomp_load8(Ctx *c, uint64_t a) {{ (void)c; return mem[a]; }}
static uint64_t recomp_load64(Ctx *c, uint64_t a) {{ uint64_t v; (void)c; memcpy(&v, mem + a, 8); return v; }}
static void recomp_store8(Ctx *c, uint64_t a, uint64_t v) {{ (void)c; mem[a] = (uint8_t)v; }}
static void put64(uint64_t a, uint64_t v) {{ memcpy(mem + a, &v, 8); }}
int rx_option(int id, int otherwise) {{ return id == HOS_OPT_SKIP_OPENING ? opt : otherwise; }}
void hos_log(const char *fmt, ...) {{
    va_list ap; va_start(ap, fmt);
    vsnprintf(said + strlen(said), sizeof said - strlen(said), fmt, ap);
    va_end(ap);
    strncat(said, "\n", sizeof said - strlen(said) - 1);
}}
#define CHECK(x) do {{ if (!(x)) {{ fprintf(stderr, "FAIL %d: %s\n", __LINE__, #x); return 1; }} }} while (0)
{opening}
int main(void) {{
    Ctx c; memset(&c, 0, sizeof c);
    memcpy(mem + 0x100, "Opening_Introduction", 21);
    memcpy(mem + 0x180, "Opening_LoadingKokiriForest", 28);
    memcpy(mem + 0x200, "Meet_DekuTree", 13);
    memcpy(mem + 0x280, "Dm_Kokiri_0000", 14);
    memcpy(mem + 0x300, "Dm_Kokiri_0030", 14);
    memcpy(mem + 0x380, "Dm_Kokiri_0070", 14);
    hos_oot_opening(0, 3);
    opt = 0;
    c.x[1] = 0x1000; put64(0x1000 + 6800, 0x100);
    CHECK(hos_oot_opening(&c, 0) == 0 && said[0] == 0);
    c.x[0] = 0x500; c.x[1] = 0x400; put64(0x400, 0x280); mem[0x500 + 0xf25] = 0xff;
    CHECK(hos_oot_opening(&c, 1) == 0 && mem[0x500 + 0xf25] == 0xff);
    opt = 1;
    c.x[1] = 0x1000; put64(0x1000 + 6800, 0x100);
    CHECK(hos_oot_opening(&c, 0) == 0);
    CHECK(strstr(said, "opening skip: Opening_Introduction request sent\n") != 0);
    said[0] = 0;
    CHECK(hos_oot_opening(&c, 0) == 0 && said[0] == 0);
    c.x[0] = 0x500; c.x[1] = 0x400; put64(0x400, 0x280); mem[0x500 + 0xf25] = 0xff;
    CHECK(hos_oot_opening(&c, 1) == 1 && mem[0x500 + 0xf25] == 0xfe);
    put64(0x400, 0x300); mem[0x500 + 0xf25] = 0xff;
    CHECK(hos_oot_opening(&c, 1) == 1 && mem[0x500 + 0xf25] == 0xfe);
    put64(0x400, 0x380); mem[0x500 + 0xf25] = 0xff;
    CHECK(hos_oot_opening(&c, 1) == 0 && mem[0x500 + 0xf25] == 0xff);
    c.x[0] = 7;
    CHECK(hos_oot_opening(&c, 2) == 1 && c.x[0] == 1);
    c.x[1] = 0x1000; put64(0x1000 + 6800, 0x200);
    CHECK(hos_oot_opening(&c, 0) == 0 && strstr(said, "Meet_DekuTree") == 0);
    c.x[0] = 7;
    CHECK(hos_oot_opening(&c, 2) == 0 && c.x[0] == 7);
    hos_oot_opening(&c, 3);
    c.x[0] = 7;
    CHECK(hos_oot_opening(&c, 2) == 0 && c.x[0] == 7);
    puts("ok");
    return 0;
}}
"#
    );
    fs::write(&file, fixture).expect("write fixture");
    let build = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(&file)
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("run the C compiler");
    assert!(build.success(), "{cc} failed on {}", file.display());
    let run = Command::new(&exe).output().expect("run the fixture");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
    let _ = fs::remove_dir_all(dir);
}
