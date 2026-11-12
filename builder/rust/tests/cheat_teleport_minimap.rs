// The debug teleport's minimap repair survives a save and reload (host/hos_main.c: oot_teleport; upstream
// NotRexed/Hookshot 21e0eeca and 59316c46, ported here in c9b2d8ea). An area's saved MapIconDeactivateByAreaActor flag
// can outlive a debug teleport; clearing only its live byte restores the HUD but leaves the save's copy hidden, so the
// teleport must use the game's queued Bool setter, which owns both. The fixture compiles the real teleport over a
// stand-in game and checks an accepted teleport, a visible minimap, refusals, a declined request and a missing setter.
// Linux only (the stand-in maps the guest's addresses as Linux does); no game or GPU is needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn compiler() -> Option<String> {
    let candidates = std::env::var("CC")
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
#include <sys/mman.h>

typedef struct { uint64_t x[32]; } Ctx;
typedef void (*A64Fn)(Ctx *);
static uint8_t *guest;   /* (the guest's first 4 GB, untouched pages cost nothing) */
#define A64_PTR(a) (guest + (uint64_t)(a))
static uint8_t recomp_load8(Ctx *c, uint64_t a) { (void)c; return *A64_PTR(a); }
static uint32_t recomp_load32(Ctx *c, uint64_t a) { (void)c; uint32_t v; memcpy(&v, A64_PTR(a), 4); return v; }
static uint64_t recomp_load64(Ctx *c, uint64_t a) { (void)c; uint64_t v; memcpy(&v, A64_PTR(a), 8); return v; }
static void recomp_store8(Ctx *c, uint64_t a, uint8_t v) { (void)c; *A64_PTR(a) = v; }
static void recomp_store32(Ctx *c, uint64_t a, uint32_t v) { (void)c; memcpy(A64_PTR(a), &v, 4); }
static void recomp_store64(Ctx *c, uint64_t a, uint64_t v) { (void)c; memcpy(A64_PTR(a), &v, 8); }
static void recomp_store64_sp(Ctx *c, uint64_t a, uint64_t v) { recomp_store64(c, a, v); }
static int64_t clock_ns = 1000000000000LL;
static int64_t hos_now_ns(void) { return clock_ns; }
static uint64_t hos_sys_alloc(uint64_t size, uint64_t align) { (void)size, (void)align; return 0x30000000ULL; }
static char said[4096];
static void hos_log(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    const size_t n = strlen(said);
    vsnprintf(said + n, sizeof said - n, fmt, ap);
    va_end(ap);
    strncat(said, "\n", sizeof said - strlen(said) - 1);
}

/* the stand-in game: GameDataMgr at MGR (its flags' table at +32, 8 slots; the bools at +24, 24 bytes each), the event
   manager at EVENTS (+104: the event playing) */
enum { MGR = 0x20000000, TABLE = 0x20001000, BOOLS = 0x20002000, EVENTS = 0x21000000, SLOTS = 8, ENTRIES = 0x22000000,
       NAMES = 0x23000000 };
static const uint32_t flags[3] = {0xcea220d3u, 0xef2b60c2u, 0x9a4dd779u};   /* IsChildhood, IsLoadingBanc, the minimap's */
static int requests;
static void game_get_bool(Ctx *c) {   /* f_8022d9c0(mgr, out, hash) */
    c->x[0] = 0;
    for (uint32_t i = 0; i < 3; i++)
        if (flags[i] == (uint32_t)c->x[2]) *A64_PTR(c->x[1]) = *A64_PTR(BOOLS + i * 24 + 22), c->x[0] = 1;
}
static int set_available = 1, set_calls, set_args_bad, pending, pending_value, saved_hidden;
static void game_set_bool(Ctx *c) {
    set_args_bad |= c->x[0] != MGR || c->x[1] > 1 || c->x[2] != flags[2];
    set_calls++, pending = 1, pending_value = (int)c->x[1];
    c->x[0] = 0xdead, c->x[2] = 0xbeef;   /* guest calls may clobber registers */
}
static void game_tick(void) {
    if (pending) {
        *A64_PTR(BOOLS + 2 * 24 + 22) = (uint8_t)pending_value;
        saved_hidden = pending_value;
        pending = 0;
    }
}
static void game_make_arg(Ctx *c) {   /* an empty EventRequestArg: 8 entries with room for their names, and the event's name */
    const uint64_t arg = c->x[0];
    recomp_store32(c, arg, 8), recomp_store64(c, arg + 8, ENTRIES), recomp_store32(c, arg + 16, 0);
    for (uint32_t i = 0; i < 8; i++)
        recomp_store64(c, ENTRIES + i * 200 + 8, NAMES + i * 64), recomp_store32(c, ENTRIES + i * 200 + 16, 64);
    recomp_store64(c, arg + 6800, NAMES + 0x1000), recomp_store32(c, arg + 6808, 64);
}
static uint64_t answer = 1;   /* the event manager's answer: 0 when it does not take the request */
static void game_request(Ctx *c) { requests++, c->x[0] = answer; }
static void game_nothing(Ctx *c) { (void)c; }
static A64Fn a64_lookup(uint64_t a) {
    return a == 0x8022d9c0ULL ? game_get_bool : a == 0x806a2b20ULL ? (set_available ? game_set_bool : NULL) :
           a == 0x80d99f10ULL ? game_make_arg : a == 0x80d9a820ULL ? game_request :
           a == 0x802dc440ULL || a == 0x8022b6f0ULL || a == 0x80cd0ea0ULL ? game_nothing : NULL;
}
static int oot_gamedata_bool(Ctx *c, uint32_t hash, int *value);
static int oot_gamedata_set_bool(Ctx *c, uint32_t hash, int value);
"#;

const RUN: &str = r#"
static void game_start(void) {
    Ctx c = {0};
    guest = mmap(NULL, 1ULL << 32, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (guest == MAP_FAILED) exit(3);
    recomp_store64(&c, OOT_GAMEDATA_SLOT, 0x1f000000), recomp_store64(&c, 0x1f000000, MGR);
    recomp_store64(&c, OOT_EVENTMGR_SLOT, 0x1f000100), recomp_store64(&c, 0x1f000100, EVENTS);
    recomp_store64(&c, MGR + 32, TABLE), recomp_store32(&c, MGR + 44, SLOTS);
    recomp_store64(&c, MGR + 24, BOOLS), recomp_store32(&c, MGR + 16, 3);
    for (uint32_t i = 0; i < 3; i++) {   /* (open addressing from hash % size, as oot_gamedata_set_bool walks it) */
        uint32_t slot = flags[i] % SLOTS;
        while (recomp_load32(&c, TABLE + slot * 8)) slot = (slot + 1) % SLOTS;
        recomp_store32(&c, TABLE + slot * 8, flags[i]), recomp_store32(&c, TABLE + slot * 8 + 4, i);
    }
    *A64_PTR(BOOLS + 0 * 24 + 22) = 1;   /* a child */
}
static int hidden(void) { return *A64_PTR(BOOLS + 2 * 24 + 22); }
static void hide(int on) { *A64_PTR(BOOLS + 2 * 24 + 22) = (uint8_t)on; saved_hidden = on; }
static int failed;
static void check(int ok, const char *what) {
    if (!ok) printf("FAIL: %s\n", what), failed = 1;
}

int main(void) {
    Ctx c = {0};
    for (unsigned i = 0; i < 32; i++) c.x[i] = 0x12340000ULL + i;
    c.x[31] = 0x40000000ULL;
    const Ctx original = c;
    game_start();
    const char *line = "the area left had hidden the minimap";

    hide(1);
    check(oot_teleport(&c, 0x50000000ULL, 1) == 1 && requests == 1, "the teleport was not sent");
    check(set_calls == 1 && pending && !set_args_bad, "the repair did not use the engine's Bool setter (mgr, value, hash)");
    check(hidden() && saved_hidden, "the repair wrote live state outside the engine setter");
    game_tick();   /* the game's normal update processes its queued changes before a save */
    check(!hidden(), "a teleport out of an area that hid the minimap left it hidden (MapIconDeactivateByAreaActor still set)");
    check(strstr(said, line) != NULL, "the log does not say the minimap was shown again");
    check(!saved_hidden, "a native save after teleport kept the minimap hidden");
    *A64_PTR(BOOLS + 2 * 24 + 22) = (uint8_t)saved_hidden;   /* a reload of the serialized value */
    check(!hidden(), "the minimap disappeared again after a save and reload");
    check(!memcmp(&c, &original, sizeof c), "the game's registers were not put back");

    clock_ns += 9000000000LL, said[0] = 0;
    check(oot_teleport(&c, 0x50000000ULL, 3) == 1 && requests == 2 && !hidden(), "a teleport with the minimap shown");
    check(set_calls == 1 && !pending && !saved_hidden, "a visible minimap queued a redundant repair");
    check(strstr(said, line) == NULL, "the log speaks of the minimap when nothing hid it");

    clock_ns += 9000000000LL, hide(1);
    recomp_store64(&c, EVENTS + 104, 0x51000000ULL);   /* an event is playing: the teleport waits */
    check(oot_teleport(&c, 0x50000000ULL, 1) == 0 && requests == 2, "a teleport was sent while an event played");
    check(hidden(), "a teleport that was refused showed the minimap of the area Link is still in");
    check(set_calls == 1 && !pending && saved_hidden, "a refused teleport changed the saved state");
    recomp_store64(&c, EVENTS + 104, 0);

    check(oot_teleport(&c, 0x50000000ULL, 1) == 1 && requests == 3, "the teleport after the event");
    game_tick();
    check(set_calls == 2 && !hidden() && !saved_hidden, "the teleport after the event did not repair both values");
    hide(1);   /* (the area he arrived in hides its own) */
    clock_ns += 1000000000LL;
    check(oot_teleport(&c, 0x50000000ULL, 3) == 0 && requests == 3, "a second teleport was sent a second after the first");
    check(hidden(), "a teleport that was ignored showed the minimap");
    check(set_calls == 2 && !pending && saved_hidden, "a throttled teleport changed the saved state");

    clock_ns += 9000000000LL, answer = 0;   /* sent, but the event manager does not take it: the map does not change */
    check(oot_teleport(&c, 0x50000000ULL, 1) == 1 && requests == 4, "the teleport the game turned down was not sent");
    check(hidden(), "a teleport the game turned down showed the minimap of the area Link is still in");
    check(set_calls == 2 && !pending && saved_hidden, "a declined request changed the saved state");

    clock_ns += 9000000000LL, answer = 1, set_available = 0, said[0] = 0;
    check(oot_teleport(&c, 0x50000000ULL, 1) == 1 && requests == 5, "the teleport with no setter binding was not sent");
    check(hidden() && saved_hidden && set_calls == 2 && !pending, "a missing setter fell back to a live-only write");
    check(strstr(said, "restore queued") == NULL, "a missing setter logged a successful repair");
    check(strstr(said, "the game's Bool setter not found, left hidden") != NULL, "a missing setter was not named in the log");
    if (!failed) puts("ok");
    return failed;
}
"#;

#[test]
fn a_teleport_repairs_the_minimap_flag_for_the_save_too() {
    if !cfg!(target_os = "linux") {
        // (the stand-in maps the guest's addresses as Linux does)
        return;
    }
    let Some(cc) = compiler() else {
        eprintln!("skipping the minimap fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
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
    let code_start = source
        .find("#define OOT_GAMEDATA_SLOT")
        .expect("the game data slot");
    let setter = source
        .find("static int oot_gamedata_set_bool(Ctx *c, uint32_t hash, int value) {")
        .expect("oot_gamedata_set_bool");
    let mut end = setter + source[setter..].find('{').unwrap() + 1;
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
        &source[code_start..end],
        RUN
    );
    let dir = std::env::temp_dir().join(format!(
        "hookshot-minimap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let source_path = dir.join("minimap.c");
    let exe = dir.join("minimap");
    fs::write(&source_path, fixture).expect("write fixture");
    let build = Command::new(&cc)
        .args(["-std=gnu11", "-O1", "-w"])
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
    let run = Command::new(&exe).output().expect("run the minimap fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
}
