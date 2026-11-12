"""The prompts' font is found at its stream's start as well as behind its 8-byte header (host/prompts.c hos_prompts_glyph).

On native Windows the game keeps the served External.bfotf in a stream the file's size, with the font from the stream's
start: the old code read from base + 8, found the OpenType header's own entrySelector and rangeShift, and never bound the
font, so a face or controls change waited for a restart. The function now looks behind the 8-byte header when the stream
is the file's size, then at the stream's start; whichever holds the font's head is where FreeType reads it. A stream whose
bytes are not readable yet is looked at again, and a stream with neither head is another font.

The fixture cuts hos_prompts_glyph and the font record out of the source and lays out a guest window with the face, its
stream and the font where the game keeps them.

python -I -B tests/test_prompt_font_find.py [--cc gcc]
"""
import argparse
import subprocess
import tempfile
from pathlib import Path

RUN_ALL_ARGS = ["--cc", "gcc"]

ROOT = Path(__file__).resolve().parents[1]
PROMPTS = ROOT / "host" / "prompts.c"


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
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); } } while (0)
enum { WINDOW = 0x10000 };
static uint8_t guest[WINDOW];   /* (guest addresses: offsets into it, from BASE) */
#define BASE 0x100000
static void *hos_guest_range(uint64_t a, uint64_t n) {
    return a >= BASE && a - BASE < WINDOW && n <= WINDOW - (a - BASE) ? guest + (a - BASE) : NULL;
}
static void hos_log(const char *fmt, ...) { (void)fmt; }
@FONT@
@GLYPH@

static int failures;
static void check(int ok, const char *what) { if (!ok) { fprintf(stderr, "FAIL: %s\n", what); failures++; } }
static uint8_t *at(uint64_t a) { return (uint8_t *)hos_guest_range(a, 1); }

/* (the face and its stream: slot -> face -> stream {base, size}) */
static uint64_t face_at(uint64_t slot, uint64_t face, uint64_t base, uint64_t size) {
    memcpy(at(slot + 8), &face, 8);
    uint64_t stream = face + 0x300, words[2] = {base, size};
    memcpy(at(face + 192), &stream, 8);
    memcpy(at(stream), &words, sizeof words);
    return face;
}
static void run(uint64_t slot) { hos_prompts_glyph(slot); }

int main(void) {
    uint8_t head[64];
    for (int i = 0; i < 64; i++) head[i] = (uint8_t)(0x40 + i);
    memcpy(font.head, head, sizeof head);
    font.n = 4096;   /* (the served file's size; no header's own size is read) */

    /* 1. the stream is the file's size, the font behind its 8-byte header (as on Linux) */
    const uint64_t f1 = BASE + 0x1000, b1 = BASE + 0x2000;
    face_at(BASE + 0x100, f1, b1, font.n);
    memcpy(at(b1), "HEADER!!", 8);
    memcpy(at(b1 + 8), head, sizeof head);
    run(BASE + 0x100);
    check(font.at == b1 + 8, "a font behind its 8-byte header is not found");

    /* 2. the stream is the file's size and the font starts at its start (native Windows) */
    font.at = 0;
    const uint64_t f2 = BASE + 0x3000, b2 = BASE + 0x4000;
    face_at(BASE + 0x200, f2, b2, font.n);
    memcpy(at(b2), head, sizeof head);
    run(BASE + 0x200);
    check(font.at == b2, "a font at its stream's start is not found");

    /* 3. the stream is the file's size less its header, the font at its start (as before) */
    font.at = 0;
    const uint64_t f3 = BASE + 0x5000, b3 = BASE + 0x6000;
    face_at(BASE + 0x300, f3, b3, font.n - 8);
    memcpy(at(b3), head, sizeof head);
    run(BASE + 0x300);
    check(font.at == b3, "a font in a stream of the size without the header is not found");

    /* 4. not readable yet: looked at again, not taken for another font */
    font.at = 0;
    const uint64_t f4 = BASE + 0x7000, b4 = BASE + 0x8000;
    face_at(BASE + 0x400, f4, 0x900000, font.n);   /* (outside the guest window) */
    run(BASE + 0x400);
    check(font.at == 0, "an unreadable stream is taken as the font");
    uint64_t words[2] = {b4, font.n};
    memcpy(at(f4 + 0x300), &words, sizeof words);
    memcpy(at(b4), head, sizeof head);
    run(BASE + 0x400);
    check(font.at == b4, "an unreadable stream is not looked at again");

    /* 5. another font of the size: not looked at again while its glyphs follow each other */
    font.at = 0;
    const uint64_t f5 = BASE + 0x9000, b5 = BASE + 0xa000;
    face_at(BASE + 0x500, f5, b5, font.n);
    memset(at(b5), 0xAA, 128);
    run(BASE + 0x500);
    check(font.at == 0, "another font of the size is taken as the served font");
    memcpy(at(b5), head, sizeof head);
    run(BASE + 0x500);
    check(font.at == 0, "another font of the size is looked at again at once");

    if (failures) return 1;
    printf("ok: the font is found behind a header, at a stream's start, and left for later while unread\n");
    return 0;
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cc", default="gcc")
    p.add_argument("--source", type=Path, default=PROMPTS)
    a = p.parse_args()
    src = a.source.read_text(encoding="utf-8")
    font = block(src, "static struct {\n    volatile uint64_t at;") + " font;"
    check("uint8_t head[64];" in font and "volatile uint64_t at;" in font, "host/prompts.c has no font record")
    glyph = block(src, "void hos_prompts_glyph(uint64_t slot) {")
    check("base[0] + (base[1] == font.n ? 8 : 0)" in glyph, "hos_prompts_glyph no longer looks behind the header first")
    check("for (uint64_t a = base[0]" in glyph, "hos_prompts_glyph does not try the stream's start")
    text = HARNESS.replace("@FONT@", font).replace("@GLYPH@", glyph)
    with tempfile.TemporaryDirectory(prefix="prompt-font-find-") as tmp:
        c, exe = Path(tmp) / "font.c", Path(tmp) / "font.exe"
        c.write_text(text, encoding="utf-8")
        subprocess.run([a.cc, "-std=c11", "-Wall", "-Wextra", "-Werror", str(c), "-o", str(exe)], check=True)
        subprocess.run([str(exe)], check=True)


if __name__ == "__main__":
    main()
