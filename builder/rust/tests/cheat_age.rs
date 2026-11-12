// The Debug tab's Switch age, game side (host/hos_main.c: oot_toggle_age, upstream NotRexed/Hookshot b157c3e8), game-free:
// the real function and destination table are compiled over a stand-in game. It flips Sequence_IsChildhood, reloads the
// map through the teleport, and puts the age back when the teleport is refused. A machine with no C compiler skips it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler() -> Option<String> {
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

const STAND_IN: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>

typedef struct { uint64_t x[32]; } Ctx;
typedef void (*A64Fn)(Ctx *);

static char said[4096];
static void hos_log(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    const size_t n = strlen(said);
    vsnprintf(said + n, sizeof said - n, fmt, ap);
    va_end(ap);
}

static struct { const char *why, *banc; } teleport;
static int fake_have_age = 1, fake_child = 1, fake_write_ok = 1, fake_teleport_ok = 1;
static int writes, wrote_value;
static unsigned teleported_to;
static const char *fake_map = "KokiriForest_Child";

static int oot_gamedata_bool(Ctx *c, uint32_t hash, int *value) {
    (void)c;
    if (hash != 0xcea220d3u) return 0;
    *value = fake_child;
    return fake_have_age;
}
static int oot_gamedata_string(Ctx *c, uint32_t hash, char *buf, size_t size) {
    (void)c;
    if (hash != 0x1d6189dau) return 0;
    snprintf(buf, size, "%s", fake_map);
    return 1;
}
static int oot_gamedata_set_bool(Ctx *c, uint32_t hash, int value) {
    (void)c;
    if (hash != 0xcea220d3u) return 0;
    writes++, wrote_value = value;
    return fake_write_ok;
}
static int oot_teleport(Ctx *c, uint64_t actor, unsigned index) {
    (void)c, (void)actor;
    teleported_to = index;
    return fake_teleport_ok;
}
"#;

const RUN: &str = r#"
#define CHECK(x) do { if (!(x)) { printf("FAIL %d: %s\n", __LINE__, #x); return 1; } } while (0)

int main(void) {
    Ctx c = {0};

    // (Link's age is not in the game's data: no flip, and the bridge is told why)
    fake_have_age = 0, said[0] = 0;
    CHECK(oot_toggle_age(&c, 1) == 0);
    CHECK(strstr(said, "Sequence_IsChildhood not found") && teleport.why &&
          strstr(teleport.why, "not in the game's data"));

    // (the age could not be written: no teleport, and the bridge is told why)
    fake_have_age = 1, fake_write_ok = 0, writes = 0, said[0] = 0;
    CHECK(oot_toggle_age(&c, 1) == 0);
    CHECK(writes == 1 && strstr(said, "could not write") && teleport.why &&
          strstr(teleport.why, "could not be written"));

    // (the teleport is refused: the age is put back, and the teleport's reason reaches the bridge)
    fake_write_ok = 1, fake_teleport_ok = 0, fake_child = 1, fake_map = "KokiriForest_Child", writes = 0;
    CHECK(oot_toggle_age(&c, 1) == 0);
    CHECK(teleported_to == 0 && writes == 2 && wrote_value == 1 && teleport.why);
    CHECK(strstr(said, "age toggle undone"));

    // (the map reloads: the age flips once and the destination for the other age is asked for)
    fake_teleport_ok = 1, fake_child = 1, fake_map = "KokiriForest_Child", writes = 0;
    CHECK(oot_toggle_age(&c, 1) == 1);
    CHECK(teleported_to == 0 && writes == 1 && wrote_value == 0);

    // (a map with both ages: the same destination, the adult's map; a map not in the table: the Market)
    fake_child = 0, fake_map = "HyruleField_Adult", writes = 0;
    CHECK(oot_toggle_age(&c, 1) == 1 && teleported_to == 1 && wrote_value == 1);
    fake_child = 1, fake_map = "NoSuchMap", writes = 0;
    CHECK(oot_toggle_age(&c, 1) == 1 && teleported_to == 3 && wrote_value == 0);

    // (the adult at Gerudo Fortress: no child map there, so the Market, where both ages are)
    fake_child = 0, fake_map = "GerudoFortress_Adult", writes = 0;
    CHECK(oot_toggle_age(&c, 1) == 1 && teleported_to == 3 && wrote_value == 1);

    puts("ok");
    return 0;
}
"#;

#[test]
fn age_toggle_flips_the_saved_flag_and_undoes_a_refused_reload() {
    let repo = root();
    let source = fs::read_to_string(repo.join("host/hos_main.c")).expect("host/hos_main.c");
    let table_start = source
        .find("typedef struct {\n    const char *child_banc")
        .expect("the destination type");
    let table_at = source[table_start..]
        .find("static const OotDestination oot_destinations[] = {")
        .expect("the destination table")
        + table_start;
    let table_end = source[table_at..]
        .find("\n};")
        .expect("the destination table's end")
        + table_at
        + 3;
    let toggle_start = source
        .find("static int oot_toggle_age(Ctx *c, uint64_t actor) {")
        .expect("oot_toggle_age");
    let mut end = toggle_start + source[toggle_start..].find('{').unwrap() + 1;
    let bytes = source.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    let fixture = format!(
        "{}\n{}\n{}\n{}",
        STAND_IN,
        &source[table_start..table_end],
        &source[toggle_start..end],
        RUN
    );
    let Some(cc) = compiler() else {
        eprintln!("skipping the age toggle fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-age-toggle-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let source_path = dir.join("age_toggle.c");
    let exe = dir.join(if cfg!(windows) {
        "age_toggle.exe"
    } else {
        "age_toggle"
    });
    fs::write(&source_path, fixture).expect("write fixture");
    let build = Command::new(&cc)
        .args(["-std=gnu11", "-Wall", "-Wextra", "-Werror"])
        .arg(&source_path)
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C compiler");
    assert!(
        build.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe)
        .output()
        .expect("run the age toggle fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}
