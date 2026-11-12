"""A directory's listing is read, counted and closed under one lock (host/nn_fs.c).

ReadDirectory and GetDirectoryEntryCount walk a directory handle: its FindNextFileA state and the mods' extra entries. A
CloseDirectory on another guest thread used to FindClose and free that handle in the middle of the walk, a use-after-free
(the save folder's listing is where the game's threads touch one directory). The fixture cuts the three handlers, their
lock and the handle table's get/free out of the source and compiles them over a find API that checks the directory lock is
held through each walk, and a last read of a closed handle that must reach the "bad handle" path, not freed memory.

python -I -B tests/test_fs_dir_lock.py [--cc gcc]
"""
import argparse
import subprocess
import tempfile
from pathlib import Path

RUN_ALL_ARGS = ["--cc", "gcc"]

ROOT = Path(__file__).resolve().parents[1]
FS = ROOT / "host" / "nn_fs.c"


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
#include <setjmp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef void *HANDLE;
typedef unsigned long DWORD;
typedef int BOOL;
typedef int SRWLOCK;
#define SRWLOCK_INIT 0
#define INVALID_HANDLE_VALUE ((HANDLE)(intptr_t)-1)
#define FILE_ATTRIBUTE_DIRECTORY 0x10
#define HOS_FS_PATH 1024
typedef struct { DWORD dwFileAttributes, nFileSizeHigh, nFileSizeLow; char cFileName[260]; } WIN32_FIND_DATAA;
typedef struct { uint64_t x[8]; } Ctx;
#define HLE(sym) void imp_##sym(Ctx *c)
#define ARG(n) (c->x[n])
#define RET(v) (c->x[0] = (uint64_t)(v))
/* (the guest's entries are written at these addresses: the window stands for the game's memory) */
enum { GUEST = 0x100000 };
static uint8_t guest[0x10000];
#define GP(a) ((void *)(guest + (uintptr_t)(a) - GUEST))
#define G64(a) (*(uint64_t *)GP(a))
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

static SRWLOCK *dir_lock_at;   /* (&dir_lock, once the cut code has made it) */
static int dir_locked, handle_locked, fatal;
static jmp_buf fatal_jmp;
static void hos_fatal(Ctx *c, const char *what, ...) { (void)c, (void)what; fatal = 1; longjmp(fatal_jmp, 1); }

/* (the cut code's lock: the handle table's own lock must not be mistaken for it) */
static void AcquireSRWLockExclusive(SRWLOCK *l) {
    CHECK(!(l == dir_lock_at ? dir_locked : handle_locked));
    if (l == dir_lock_at) dir_locked = 1; else handle_locked = 1;
}
static void ReleaseSRWLockExclusive(SRWLOCK *l) {
    CHECK(l == dir_lock_at ? dir_locked : handle_locked);
    if (l == dir_lock_at) dir_locked = 0; else handle_locked = 0;
}

/* the find API: every walk must have the directory lock, never only the start of the handler */
static int next_left;
static void fill(WIN32_FIND_DATAA *fd, const char *name, DWORD attributes, DWORD size) {
    memset(fd, 0, sizeof *fd);
    fd->dwFileAttributes = attributes, fd->nFileSizeLow = size;
    snprintf(fd->cFileName, sizeof fd->cFileName, "%s", name);
}
static HANDLE FindFirstFileA(const char *path, WIN32_FIND_DATAA *fd) {
    (void)path;
    CHECK(dir_locked);
    next_left = 1;   /* (one more after this one) */
    fill(fd, "alpha.txt", 0, 11);
    return (HANDLE)(intptr_t)0x1234;
}
static BOOL FindNextFileA(HANDLE h, WIN32_FIND_DATAA *fd) {
    (void)h;
    CHECK(dir_locked);
    if (!next_left) return 0;
    next_left = 0;
    fill(fd, "sub", FILE_ATTRIBUTE_DIRECTORY, 0);
    return 1;
}
static BOOL FindClose(HANDLE h) { (void)h; CHECK(dir_locked); return 1; }

@TYPES@
@HANDLE_STRUCT@
@HANDLES@
@GET@
@FREE@
@LOCK@
@READ@
@COUNT@
@CLOSE@

int main(void) {
    dir_lock_at = &dir_lock;
    Handle *dh = (Handle *)calloc(1, sizeof *dh);
    dh->find = (HANDLE)(intptr_t)0x55;
    dh->is_dir = 3;   /* (files and directories) */
    snprintf(dh->path, sizeof dh->path, "%s", "C:\\save\\*");
    fill(&dh->fd, "first.txt", 0, 7);
    handles[0] = dh;

    /* ReadDirectory walks the find handle and then the mods' entries, under the lock, and lets it go */
    Ctx c;
    memset(&c, 0, sizeof c);
    c.x[0] = GUEST + 0x8000, c.x[1] = GUEST, c.x[2] = 1, c.x[3] = 4;
    next_left = 1;   /* (the open's first entry "first.txt" is in dh->fd; one more follows) */
    imp__ZN2nn2fs13ReadDirectoryEPlPNS0_14DirectoryEntryENS0_15DirectoryHandleEl(&c);
    CHECK(!dir_locked && !handle_locked && !fatal);
    CHECK(G64(GUEST + 0x8000) == 2 && G64(GUEST + 0x308) == 7);   /* (first.txt, then the subdirectory) */
    CHECK(!strcmp((const char *)GP(GUEST), "first.txt") && *(uint8_t *)GP(GUEST + 0x304) == 1);

    /* GetDirectoryEntryCount walks a listing of its own, under the same lock */
    c.x[0] = GUEST + 0x8000, c.x[1] = 1;
    imp__ZN2nn2fs22GetDirectoryEntryCountEPlNS0_15DirectoryHandleE(&c);
    CHECK(!dir_locked && !handle_locked && !fatal);
    CHECK(G64(GUEST + 0x8000) == 2);

    /* CloseDirectory closes and frees the handle under it */
    c.x[0] = 1;
    imp__ZN2nn2fs14CloseDirectoryENS0_15DirectoryHandleE(&c);
    CHECK(!dir_locked && !handle_locked && !fatal);
    CHECK(handles[0] == NULL);

    /* a read that arrives after the close reaches the bad-handle check: the handle is not in the table any more */
    if (!setjmp(fatal_jmp)) {
        c.x[2] = 1;
        imp__ZN2nn2fs13ReadDirectoryEPlPNS0_14DirectoryEntryENS0_15DirectoryHandleEl(&c);
        CHECK(0);
    }
    CHECK(fatal);
    return 0;
}
'''


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--cc", default="gcc")
    p.add_argument("--source", type=Path, default=FS)
    a = p.parse_args()
    src = a.source.read_text(encoding="utf-8")
    parts = {
        "@TYPES@": block(src, "typedef struct { char name[HOS_FS_PATH]; int is_dir; uint64_t size; } ExtraEntry;") + " ExtraEntry;",
        "@HANDLE_STRUCT@": block(src, "typedef struct {\n    HANDLE h; int writable;") + " Handle;",
        "@HANDLES@": "static Handle *handles[4096];\nstatic SRWLOCK handle_lock = SRWLOCK_INIT;\n",
        "@GET@": block(src, "static Handle *get_handle(Ctx *c, uint64_t id) {"),
        "@FREE@": block(src, "static void free_handle(uint64_t id) {"),
        "@LOCK@": ("/* (hookshot) A directory's listing is read, counted and closed under one lock: a CloseDirectory on another "
                   "guest thread freed\n   the handle (and its find handle) while a ReadDirectory or GetDirectoryEntryCount "
                   "still walked it */\nstatic SRWLOCK dir_lock = SRWLOCK_INIT;\n"),
        "@READ@": block(src, "HLE(_ZN2nn2fs13ReadDirectoryEPlPNS0_14DirectoryEntryENS0_15DirectoryHandleEl) {"),
        "@COUNT@": block(src, "HLE(_ZN2nn2fs22GetDirectoryEntryCountEPlNS0_15DirectoryHandleE) {"),
        "@CLOSE@": block(src, "HLE(_ZN2nn2fs14CloseDirectoryENS0_15DirectoryHandleE) {"),
    }
    # (the lock's definition is part of the source, not spelled here twice: where it is, the handlers' shape is checked)
    at = src.index(parts["@READ@"])
    found = src[at - 400:at]
    check("static SRWLOCK dir_lock = SRWLOCK_INIT;" in found, "host/nn_fs.c has no dir_lock before ReadDirectory")
    check("AcquireSRWLockExclusive(&dir_lock);" in parts["@READ@"].split("Handle *dh")[0],
          "ReadDirectory does not take the directory lock before it looks the handle up")
    check("AcquireSRWLockExclusive(&dir_lock);" in parts["@COUNT@"].split("Handle *dh")[0],
          "GetDirectoryEntryCount does not take the directory lock before it looks the handle up")
    check("AcquireSRWLockExclusive(&dir_lock);" in parts["@CLOSE@"].split("Handle *dh")[0],
          "CloseDirectory does not take the directory lock before it looks the handle up")

    text = HARNESS
    for key, part in parts.items():
        text = text.replace(key, part)
    with tempfile.TemporaryDirectory(prefix="fs-dir-lock-") as tmp:
        c, exe = Path(tmp) / "dir.c", Path(tmp) / "dir.exe"
        c.write_text(text, encoding="utf-8")
        subprocess.run([a.cc, "-std=c11", "-Wall", "-Wextra", "-Werror", "-Wno-format-truncation",
                        "-Wno-unknown-warning-option", "-Wno-unused-function", str(c), "-o", str(exe)],
                       check=True)
        subprocess.run([str(exe)], check=True)
    print("ok: the listing, its count and its close all hold the directory lock")


if __name__ == "__main__":
    main()
