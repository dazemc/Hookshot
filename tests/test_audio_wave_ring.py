"""A near-full waveOut ring retires its oldest queued buffer (host/posix/win32_posix.c).

During shader compilation or a heavy frame stall the game writes audio faster than SDL's callback takes it. The ring used
to return MMSYSERR_ERROR at 64 queued buffers, after which the game's writes kept failing and the audio never recovered.
The fixture cuts the ring, its mixer pull and waveOutWrite/waveOutReset out of the source: it fills the ring past the
point where the old code failed, checks that every write is accepted and that the oldest buffer alone is published done
with its head advanced, then drains and resets the queue in order.

POSIX only: this is the Linux shim of waveOut; Windows uses winmm.

python -I -B tests/test_audio_wave_ring.py [--cc gcc]
"""
import argparse
import subprocess
import tempfile
from pathlib import Path

RUN_ALL_ARGS = ["--cc", "gcc"]
RUN_ALL_SKIP = {"windows": "the POSIX waveOut shim: Windows uses winmm"}

ROOT = Path(__file__).resolve().parents[1]
POSIX = ROOT / "host" / "posix" / "win32_posix.c"


def block(text, marker):
    start = text.index(marker)
    end = text.index("{", start) + 1
    depth = 1
    while depth:
        depth += (text[end] == "{") - (text[end] == "}")
        end += 1
    return text[start:end]


def check(cond, what):
    if not cond:
        raise SystemExit("FAIL: " + what)


HARNESS = r'''
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef unsigned int UINT;
typedef unsigned short WORD;
typedef uintptr_t DWORD_PTR;
typedef char *LPSTR;
typedef unsigned long DWORD;
typedef struct wavehdr_tag {
    LPSTR lpData;
    DWORD dwBufferLength, dwBytesRecorded;
    DWORD_PTR dwUser;
    volatile DWORD dwFlags;
    DWORD dwLoops;
    struct wavehdr_tag *lpNext;
    DWORD_PTR reserved;
} WAVEHDR;
typedef void *HWAVEOUT;
typedef UINT MMRESULT;
#define MMSYSERR_NOERROR 0
#define MMSYSERR_ERROR 1
#define WHDR_DONE 0x1
#define WHDR_PREPARED 0x2
#define WHDR_INQUEUE 0x10
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

@RING@
@PULL@
@WRITE@
@RESET@

enum { N = 70 };
static unsigned char data[N][4];
static WAVEHDR h[N];
static unsigned char out[N * 4];

int main(void) {
    WaveOut w;
    memset(&w, 0, sizeof w);
    pthread_mutex_init(&w.mu, NULL);
    for (int i = 0; i < N; i++) {
        memset(data[i], i, sizeof data[i]);
        h[i].lpData = (LPSTR)data[i], h[i].dwBufferLength = sizeof data[i], h[i].dwFlags = WHDR_PREPARED;
    }

    /* 60 fit; the 61st retires the oldest rather than refusing itself */
    for (int i = 0; i < 60; i++) CHECK(waveOutWrite(&w, &h[i], sizeof h[i]) == MMSYSERR_NOERROR);
    CHECK(w.count == 60 && w.head == 0 && w.pos == 0);
    CHECK((h[0].dwFlags & WHDR_INQUEUE) && !(h[0].dwFlags & WHDR_DONE));
    CHECK(waveOutWrite(&w, &h[60], sizeof h[60]) == MMSYSERR_NOERROR);
    CHECK(w.count == 60 && w.head == 1 && w.pos == 0);
    CHECK(!(h[0].dwFlags & WHDR_INQUEUE) && (h[0].dwFlags & WHDR_DONE));   /* (the oldest is the one retired) */
    CHECK((h[1].dwFlags & WHDR_INQUEUE) && !(h[1].dwFlags & WHDR_DONE));
    CHECK((h[60].dwFlags & WHDR_INQUEUE) && !(h[60].dwFlags & WHDR_DONE));

    /* the mixer takes the queued buffers in order, the retired one first: h1 .. h60 */
    memset(out, 0xFF, sizeof out);
    wave_pull(&w, out, 60 * 4);
    CHECK(w.count == 0 && w.head == 61 % 64 && w.pos == 0);
    for (int i = 0; i < 60; i++)
        CHECK(out[i * 4] == (unsigned char)(i + 1) && (h[i + 1].dwFlags & WHDR_DONE));
    CHECK(!(h[0].dwFlags & WHDR_INQUEUE) && (h[0].dwFlags & WHDR_DONE));

    /* waveOutReset releases whatever is left */
    for (int i = 0; i < 10; i++) CHECK(waveOutWrite(&w, &h[i], sizeof h[i]) == MMSYSERR_NOERROR);
    CHECK(w.count == 10);
    waveOutReset((HWAVEOUT)&w);
    CHECK(w.count == 0 && w.pos == 0);
    for (int i = 0; i < 10; i++) CHECK((h[i].dwFlags & WHDR_DONE) && !(h[i].dwFlags & WHDR_INQUEUE));
    pthread_mutex_destroy(&w.mu);
    return 0;
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cc", default="gcc")
    p.add_argument("--source", type=Path, default=POSIX)
    a = p.parse_args()
    src = a.source.read_text(encoding="utf-8")
    ring = "typedef struct {\n    pthread_mutex_t mu;\n    WAVEHDR *q[64];\n    int head, count;\n    DWORD pos;\n} WaveOut;"
    check(ring in src, "host/posix/win32_posix.c has no WaveOut ring")
    parts = {
        "@RING@": ring,
        "@PULL@": block(src, "static void wave_pull(void *user, uint8_t *out, int len) {"),
        "@WRITE@": block(src, "MMRESULT waveOutWrite(HWAVEOUT hw, WAVEHDR *h, UINT size) {"),
        "@RESET@": block(src, "MMRESULT waveOutReset(HWAVEOUT hw) {"),
    }
    check("if (w->count >= 60)" in parts["@WRITE@"], "waveOutWrite no longer retires a buffer before the ring is full")
    text = HARNESS
    for key, part in parts.items():
        text = text.replace(key, part)
    with tempfile.TemporaryDirectory(prefix="audio-wave-ring-") as tmp:
        c, exe = Path(tmp) / "ring.c", Path(tmp) / "ring.exe"
        c.write_text(text, encoding="utf-8")
        subprocess.run([a.cc, "-std=c11", "-Wall", "-Wextra", "-Werror", "-Wno-unused-function", str(c), "-o", str(exe),
                        "-pthread"], check=True)
        subprocess.run([str(exe)], check=True)
    print("ok: the ring retires its oldest buffer, keeps the newest and drains in order")


if __name__ == "__main__":
    main()
