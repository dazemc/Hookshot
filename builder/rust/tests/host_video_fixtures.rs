// Game-free checks of the host video path (host/nn_video.c, host/dav1d_av1.c, host/video_uv.h): the AV1 decoder fallback
// order (dav1d before Media Foundation when the build has it), the lazy RGBA/UV conversion and picture queue lifetime, and
// the AcquireXDecoderOutputBuffer wait (a movie's end no longer waits 245-257 ms for a picture that is not coming, upstream
// NotRexed/Hookshot cc04531b). The decoder wrapper itself is run against a synthetic lossless AV1 clip when dav1d is
// installed. Nothing here reads a game, a dump or a GPU.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// A C definition's braces, matched, from its first line.
fn function(source: &str, marker: &str) -> String {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + source[start..].find('{').expect("opening brace") + 1;
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
    source[start..end].to_string()
}

/// From `marker` to the end of the line holding `last` (a typedef with its name).
fn through(source: &str, marker: &str, last: &str) -> String {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let end = start
        + source[start..]
            .find(last)
            .unwrap_or_else(|| panic!("missing {last}"))
        + last.len();
    source[start..end].to_string()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

fn build(
    cc: &str,
    name: &str,
    source: &str,
    includes: &[&Path],
    flags: &[&str],
) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let src = dir.join("fixture.c");
    fs::write(&src, source).expect("write fixture");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let mut command = Command::new(cc);
    command.args(["-std=c11", "-O1", "-w"]);
    for include in includes {
        command.arg(format!("-I{}", include.display()));
    }
    command.arg(&src).args(flags).arg("-o").arg(&exe);
    let output = command.output().expect("run the C compiler");
    assert!(
        output.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (dir, exe)
}

fn run(exe: &Path, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(exe);
    for (name, value) in envs {
        command.env(name, value);
    }
    command.output().expect("run the fixture")
}

#[test]
fn av1_falls_back_from_dav1d_to_media_foundation_only_when_built_in() {
    let source = fs::read_to_string(root().join("host/nn_video.c")).expect("host/nn_video.c");
    let table = function(&source, "static const Av1Decoder av1_decoders[]");
    let dav1d = table.find("\"dav1d\"").expect("dav1d in the table");
    let mf = table
        .find("\"Media Foundation\"")
        .expect("Media Foundation in the table");
    assert!(
        dav1d < mf,
        "dav1d must be tried before Media Foundation: {table}"
    );
    assert!(
        table[..dav1d].contains("#ifdef HOOKSHOT_DAV1D"),
        "dav1d is only in the table when the build found it"
    );
    let init = function(
        &source,
        "static void dec_init_sized(Ctx *c, enum AVCodecID id, const char *what, int w, int h) {",
    );
    assert!(
        init.contains("i < sizeof av1_decoders / sizeof av1_decoders[0]")
            && init.contains("d->av1 = &av1_decoders[i]"),
        "dec_init_sized must try every AV1 decoder in order"
    );
    for call in ["d->av1->decode(", "d->av1->flush(", "d->av1->close("] {
        assert!(
            source.contains(call),
            "the AV1 table is not used for {call}"
        );
    }
    // (the Windows build finds dav1d in the CPM cache, links it, and compiles the wrapper only then)
    let build = fs::read_to_string(root().join("nx2/build.py")).expect("nx2/build.py");
    assert!(build.contains("def dav1d_windows()"));
    assert!(build.contains("(f != \"dav1d_av1.c\" or dav1d_windows())"));
    assert!(build.contains("libdav1d.a"));
    let rust_build =
        fs::read_to_string(root().join("builder/rust/src/host_build.rs")).expect("host_build.rs");
    assert!(rust_build.contains("fn find_dav1d_in("));
    assert!(rust_build.contains("name != \"dav1d_av1.c\" || dav1d"));
    assert!(rust_build.contains("lib/libdav1d.a"));
}

#[test]
fn lazy_rgba_keeps_nv12_and_interleaves_planar_uv() {
    let source = fs::read_to_string(root().join("host/nn_video.c")).expect("host/nn_video.c");
    let queue = function(&source, "static void queue_frame(");
    let fixture = format!(
        r##"
#ifndef _WIN32
#define _GNU_SOURCE   /* (setenv) */
#endif
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "video_uv.h"
#include "hookshot_env.h"

enum AVPixelFormat {{ AV_PIX_FMT_NV12, AV_PIX_FMT_YUV420P, AV_PIX_FMT_YUVA420P, AV_PIX_FMT_RGBA }};
#define SWS_BILINEAR 2
typedef struct AVFrame {{ int width, height, format; uint8_t *data[4]; int linesize[4]; int64_t pts; }} AVFrame;
typedef struct SwsContext SwsContext;
#define MAX_QUEUE 8
typedef struct {{ uint8_t *rgba, *nv12; int w, h; int64_t pts; int last; }} Picture;
typedef struct {{ SwsContext *sws; int sws_w, sws_h, sws_fmt; Picture queue[MAX_QUEUE]; int qhead, qn; }} Dec;
typedef long LONG;
static int sws_calls, frees;
static LONG InterlockedIncrement(volatile LONG *p) {{ return ++*p; }}
static void hos_log(const char *fmt, ...) {{ (void)fmt; }}
static SwsContext *sws_getContext(int a, int b, enum AVPixelFormat c, int d, int e, enum AVPixelFormat f, int g,
                                  void *h, void *i, void *j) {{
    (void)a, (void)b, (void)c, (void)d, (void)e, (void)f, (void)g, (void)h, (void)i, (void)j; return (SwsContext *)1;
}}
static int sws_scale(SwsContext *c, const uint8_t *const src[], const int stride[], int y, int h, uint8_t *const dst[],
                     const int dst_stride[]) {{
    (void)c, (void)src, (void)stride, (void)y, (void)h, (void)dst, (void)dst_stride;
    sws_calls++; return 0;
}}
static void sws_freeContext(SwsContext *c) {{ (void)c; }}
static void picture_free(Picture *p) {{ free(p->rgba); free(p->nv12); memset(p, 0, sizeof *p); frees++; }}

@QUEUE@

static void check(int ok, const char *what) {{ if (!ok) {{ fprintf(stderr, "FAIL: %s\n", what); exit(1); }} }}
static void set_rgba(const char *v) {{
#ifdef _WIN32
    _putenv_s("HOS_VIDEO_RGBA", v);
#else
    setenv("HOS_VIDEO_RGBA", v, 1);
#endif
}}
static AVFrame frame(int format, int w, int h, uint8_t *y, uint8_t *u, uint8_t *v, int stride) {{
    AVFrame f; memset(&f, 0, sizeof f);
    f.width = w, f.height = h, f.format = format;
    f.data[0] = y, f.data[1] = u, f.data[2] = v;
    f.linesize[0] = f.linesize[1] = f.linesize[2] = stride;
    return f;
}}
int main(void) {{
    Dec d; memset(&d, 0, sizeof d);
    uint8_t y[16 * 16], u[8 * 8], v[8 * 8];
    for (int i = 0; i < (int)sizeof y; i++) y[i] = (uint8_t)i;
    for (int i = 0; i < (int)sizeof u; i++) u[i] = (uint8_t)(0x40 + i), v[i] = (uint8_t)(0x80 + i);

    AVFrame nv = frame(AV_PIX_FMT_NV12, 16, 16, y, u, v, 16);
    queue_frame(&d, &nv);
    check(d.qn == 1 && d.queue[0].rgba == NULL, "NV12 keeps no RGBA");
    check(d.queue[0].nv12 && d.queue[0].nv12[0] == y[0] && d.queue[0].nv12[16 * 16 + 1] == u[1],
          "NV12 copies luma and chroma");
    check(sws_calls == 0, "no eager sws conversion for NV12");

    AVFrame p420 = frame(AV_PIX_FMT_YUV420P, 16, 16, y, u, v, 16);
    queue_frame(&d, &p420);
    check(d.qn == 2 && d.queue[1].rgba == NULL, "planar 4:2:0 keeps no RGBA");
    check(d.queue[1].nv12[16 * 16] == u[0] && d.queue[1].nv12[16 * 16 + 1] == v[0] &&
          d.queue[1].nv12[16 * 16 + 2] == u[1] && d.queue[1].nv12[16 * 16 + 3] == v[1],
          "planar U/V is interleaved into NV12");
    check(sws_calls == 0, "no eager sws conversion for planar 4:2:0");

    set_rgba("1");
    AVFrame eager = frame(AV_PIX_FMT_YUV420P, 16, 16, y, u, v, 16);
    queue_frame(&d, &eager);
    check(d.qn == 3 && d.queue[2].rgba != NULL, "HOS_VIDEO_RGBA=1 makes RGBA eagerly");
    check(sws_calls == 1, "the eager path uses sws_scale");
    set_rgba("");

    /* The queue is bounded: an old picture is freed, not leaked, and the newest is kept. */
    for (int i = 0; i < MAX_QUEUE + 2; i++) queue_frame(&d, &p420);
    check(d.qn == MAX_QUEUE, "the picture queue stays bounded");
    check(frees == 5, "the oldest pictures are freed when the queue is full");
    for (int i = 0; i < d.qn; i++) picture_free(&d.queue[(d.qhead + i) % MAX_QUEUE]);
    puts("ok");
    return 0;
}}
"##
    )
    .replace("@QUEUE@", &queue);
    let Some(cc) = compiler() else {
        eprintln!("skipping the lazy RGBA fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let (dir, exe) = build(
        &cc,
        "video-lazy-rgba",
        &fixture,
        &[&root().join("host"), &root().join("gpu/vk/compat")],
        &[],
    );
    let output = run(&exe, &[]);
    let _ = fs::remove_dir_all(dir);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ok"));
}

#[test]
fn movie_acquire_waits_by_the_clock_and_not_after_a_stop() {
    let source = fs::read_to_string(root().join("host/nn_video.c")).expect("host/nn_video.c");
    let defines: String = source
        .lines()
        .filter(|line| {
            line.starts_with("#define ")
                && [
                    "RESULT_NO_OUTPUT",
                    "POOL_MAGIC",
                    "DEC_MAGIC",
                    "MAX_SLOTS",
                    "MAX_QUEUE",
                    "SYNC_IDLE_TAG",
                    "SYNC_IDLE_MASK",
                ]
                .iter()
                .any(|name| line.contains(&format!("{name} ")))
        })
        .map(|line| format!("{line}\n"))
        .collect();
    let types = format!(
        "{defines}{}\n{}\n{}\n{}\n{}\n{}\nstatic volatile LONG64 submissions;\n",
        through(
            &source,
            "typedef struct {\n    int w, h, format, count;",
            "} Pool;"
        ),
        through(
            &source,
            "typedef struct { uint8_t *rgba, *nv12;",
            "} Picture;"
        ),
        through(&source, "typedef struct Pending {", "} Pending;"),
        through(
            &source,
            "typedef void (*mf_av1_picture_fn)",
            "int64_t pts);"
        ),
        through(
            &source,
            "typedef struct {\n    const char *name;",
            "} Av1Decoder;"
        ),
        through(
            &source,
            "typedef struct {\n    AVCodecContext *ctx;",
            "} Dec;"
        ),
    );
    let acquire = function(&source, "static void dec_acquire(Ctx *c) {");
    let fixture = HARNESS
        .replace("@TYPES@", &types)
        .replace("@ACQUIRE@", &acquire);
    let Some(cc) = compiler() else {
        eprintln!("skipping the movie acquire fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let (dir, exe) = build(
        &cc,
        "video-acquire",
        &fixture,
        &[&root().join("gpu/vk/compat")],
        &["-include", "hookshot_env.h"],
    );
    let output = run(&exe, &[]);
    let _ = fs::remove_dir_all(dir);
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ok"));
}

const HARNESS: &str = r##"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct AVCodecContext AVCodecContext;
typedef struct AVPacket AVPacket;
typedef struct AVFrame AVFrame;
struct SwsContext;
typedef long long LONG64;
typedef struct { int locked; } SRWLOCK;
typedef struct { uint64_t x[32]; } Ctx;
#define ARG(n) (c->x[n])
#define RET(v) (c->x[0] = (uint64_t)(v))
#define GP(a) ((void *)(uintptr_t)(a))
#define G64(a) (*(uint64_t *)GP(a))
#define G32(a) (*(uint32_t *)GP(a))
#define HOS_WRITTEN(p, n) ((void)0)
static LONG64 InterlockedCompareExchange64(volatile LONG64 *p, LONG64 x, LONG64 cmp) { (void)x, (void)cmp; return *p; }
static int video_log(void) { return 0; }
static void hos_log(const char *f, ...) { (void)f; }
static int64_t clock_ns = 1000000000;
static int64_t hos_now_ns(void) { return clock_ns; }
static void *hos_guest_range(uint64_t a, uint64_t n) { (void)a, (void)n; return NULL; }

@TYPES@

/* the thread's virtual core, as hos_vcore_away / hos_vcore_back keep it (host/hos_thread.c) */
static Dec dec;
static int held, takes, takes_under_lock, decodes, decodes_with_core, sleeps, sleeps_with_core;
int hos_vcore_away(void) { if (!held) return 0; held = 0; return 1; }
void hos_vcore_back(void) {
    if (held) return;
    held = 1, takes++;
    if (dec.lock.locked) takes_under_lock++;
}
static void AcquireSRWLockExclusive(SRWLOCK *l) { l->locked = 1; }
static void ReleaseSRWLockExclusive(SRWLOCK *l) { l->locked = 0; }
/* hos_sleep_ns as host/hos_thread.c has it: the core given up for the sleep, if one is held */
static int64_t sleep_takes_ns;   /* (what a sleep really takes, when not what was asked: a 2 ms sleep takes 4-5 ms) */
static void hos_sleep_ns(int64_t ns) {
    sleeps++;
    sleeps_with_core += held;
    const int away = hos_vcore_away();
    clock_ns += sleep_takes_ns ? sleep_takes_ns : ns;
    if (away) hos_vcore_back();
}
static Pool pool;
static void *host_of(uint64_t obj, uint32_t magic) {
    if (magic == DEC_MAGIC) return obj == 1 ? &dec : NULL;
    return magic == POOL_MAGIC && obj == 2 ? &pool : NULL;
}
static void pool_rgba(Pool *o) { (void)o; }
static void picture_free(Picture *p) { free(p->rgba), free(p->nv12); memset(p, 0, sizeof *p); }
/* the decoding (libavcodec and dav1d's waits in the real one): one picture per submission, after `late` calls */
static Pending one;
static int late, end_after;
static int stopped_now;   /* (the decoder was stopped or flushed and nothing was submitted since) */
static void decode_next(Dec *d) {
    decodes++;
    decodes_with_core += held;
    if (!d->phead) return;
    if (late > 0) { late--; return; }
    d->phead = NULL;
    if (end_after) { d->eos = 1; return; }
    Picture *p = &d->queue[(d->qhead + d->qn) % MAX_QUEUE];
    p->w = p->h = 4, p->pts = 100, p->last = 1;
    p->nv12 = (uint8_t *)calloc(1, 4 * 4 * 3 / 2);
    d->qn++;
}
static void decode_pending(Dec *d) { decode_next(d); }

@ACQUIRE@

static int bad;
static void check(int ok, const char *what) { if (!ok) { printf("FAIL: %s\n", what); bad = 1; } }
/* one AcquireXDecoderOutputBuffer(&index, &sync, decoder) by a thread that holds a core (or not) */
static uint64_t acquire(int core, int pending, int registered) {
    static uint32_t index;
    static uint64_t sync;
    for (int i = 0; i < dec.qn; i++) picture_free(&dec.queue[(dec.qhead + i) % MAX_QUEUE]);
    memset(&dec, 0, sizeof dec);
    if (!pool.nv12) pool.w = pool.h = 4, pool.count = 1, pool.nv12 = (uint8_t *)calloc(1, 4 * 4 * 3 / 2);
    dec.slots[0] = registered ? 2 : 0;
    dec.phead = dec.ptail = pending ? &one : NULL;
    dec.stopped = stopped_now;
    Ctx c;
    memset(&c, 0, sizeof c);
    c.x[0] = (uint64_t)(uintptr_t)&index, c.x[1] = (uint64_t)(uintptr_t)&sync, c.x[2] = 1;
    held = core, takes = takes_under_lock = decodes = decodes_with_core = sleeps = sleeps_with_core = 0;
    dec_acquire(&c);
    return c.x[0];
}
int main(void) {
    uint64_t r = acquire(1, 1, 1);
    check(r == 0 && dec.acquires == 1 && dec.qn == 0, "a picture decoded and handed out");
    check(decodes > 0 && decodes_with_core == 0, "the decoding runs without the core");
    check(takes == 1 && takes_under_lock == 0 && held == 1, "the core taken again once, after d->lock is let go");

    end_after = 1;
    r = acquire(1, 1, 1);
    check(r == 0 && dec.eos && decodes_with_core == 0, "nothing decoded after the last submission: decoded without the core");
    check(takes == 1 && takes_under_lock == 0 && held == 1, "and the core back after the lock");
    end_after = 0;

    r = acquire(1, 1, 0);
    check(r == RESULT_NO_OUTPUT && decodes_with_core == 0, "no free buffer: decoded without the core, no picture yet");
    check(takes == 1 && takes_under_lock == 0 && held == 1, "and the core back after the lock");

    late = 3;
    r = acquire(1, 1, 1);
    check(r == 0 && dec.acquires == 1 && sleeps == 3 && decodes_with_core == 0, "a decode after three 2-ms retries, without the core");
    check(sleeps_with_core == 0 && takes == 1 && takes_under_lock == 0 && held == 1,
          "no core taken for the retries' sleeps; taken once, after the lock");
    late = 0;

    r = acquire(0, 1, 1);
    check(r == 0 && held == 0 && takes == 0, "a thread that held no core takes none");

    /* the wait for a picture another thread's submission may bring: ~100 ms by the clock, whatever a sleep takes, and none
       after a Stop or a Flush (a movie's end: the thread its teardown joins sat here for 245-257 ms) */
    r = acquire(1, 0, 1);
    check(r == 0 && sleeps == 50 && sleeps_with_core == 0, "nothing decoded yet: 100 ms of 2-ms sleeps, then the buffer again");
    sleep_takes_ns = 5000000;
    const int64_t before = clock_ns;
    r = acquire(1, 0, 1);
    check(r == 0 && sleeps == 20 && clock_ns - before == 100000000, "sleeps that take 5 ms: still 100 ms, not 50 tries (250 ms)");
    stopped_now = 1;
    r = acquire(1, 0, 1);
    check(r == 0 && sleeps == 0 && dec.acquires == 1, "stopped and nothing submitted since: no wait, the buffer again at once");
    check(takes == 1 && takes_under_lock == 0 && held == 1, "and the core back after the lock");
    stopped_now = 0, sleep_takes_ns = 0;
    late = 3;
    r = acquire(1, 1, 1);
    check(r == 0 && sleeps == 3, "a pending submission is still waited for");
    late = 0;
    if (!bad) puts("ok");
    return bad;
}
"##;

/// A dav1d prefix: HOOKSHOT_DAV1D, the CPM cache's, build-linux's dependencies, or the system's.
fn dav1d_prefix() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("HOOKSHOT_DAV1D") {
        roots.push(PathBuf::from(path));
    }
    for cache in [
        std::env::var_os("RX_CPM").map(PathBuf::from),
        Some(root().join("build-linux/deps/ffmpeg")),
        Some(root().join("cpm")),
    ]
    .into_iter()
    .flatten()
    {
        let folder = cache.join("dav1d");
        if let Ok(entries) = fs::read_dir(&folder) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            roots.extend(names.into_iter().rev().map(|name| folder.join(name)));
        }
    }
    roots.extend([PathBuf::from("/usr/local"), PathBuf::from("/usr")]);
    roots
        .into_iter()
        .find(|root| root.join("include/dav1d/dav1d.h").is_file())
}

#[test]
fn dav1d_decodes_a_lossless_av1_stream_and_drains_at_the_end() {
    let Some(prefix) = dav1d_prefix() else {
        eprintln!(
            "skipping the dav1d fixture: no dav1d headers (HOOKSHOT_DAV1D, CPM, build-linux or /usr)"
        );
        return;
    };
    let Some(cc) = compiler() else {
        eprintln!("skipping the dav1d fixture: no C compiler (CC, cc, gcc or clang)");
        return;
    };
    let dir = scratch("video-dav1d");
    let src = root().join("tests/dav1d_av1_test.c");
    let exe = dir.join(if cfg!(windows) {
        "fixture.exe"
    } else {
        "fixture"
    });
    let static_lib = prefix.join("lib/libdav1d.a");
    let mut command = Command::new(&cc);
    command
        .args(["-std=c11", "-O1", "-w"])
        .arg(format!("-I{}", prefix.join("include").display()))
        .arg(&src)
        .arg(root().join("host/dav1d_av1.c"))
        .arg("-o")
        .arg(&exe);
    if static_lib.is_file() && prefix != Path::new("/usr") {
        command.arg(&static_lib).args(["-lpthread", "-lm"]);
    } else {
        command.arg("-ldav1d");
    }
    let built = command.output().expect("run the C compiler");
    assert!(
        built.status.success(),
        "{cc}: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let clip = root().join("tests/dav1d_av1/lossless_64x48.ivf");
    for threads in ["1", "4"] {
        let output = Command::new(&exe)
            .arg(&clip)
            .env("HOS_DAV1D_THREADS", threads)
            .env_remove("HOS_NO_DAV1D")
            .output()
            .expect("run the dav1d fixture");
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("ok"));
    }
    let off = Command::new(&exe)
        .arg(&clip)
        .env("HOS_NO_DAV1D", "1")
        .output()
        .expect("run the dav1d fixture with HOS_NO_DAV1D");
    let _ = fs::remove_dir_all(dir);
    assert!(
        !off.status.success(),
        "HOS_NO_DAV1D=1 must leave the decoder unopened (the fixture's CHECK(m))"
    );
}
