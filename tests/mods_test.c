/* RomFS mods in the emulator layout (host/mods.c): the folders a mod may sit in, which one wins when two replace the
   same file, and the exact resource-table lookup the button prompts use. The fixture tree is made by
   builder/rust/tests/host_c_fixtures.rs; HOS_MODS points at its root. No game file or game-made data.

   host/mods.c is built with -DHOS_H (the test supplies the few declarations it needs) and, on Linux, with
   host/posix/win32_posix.c for the file calls. Exit 0: every case as it should be. */
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void hos_log(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}

/* prompts.c's side: a replacement for a path, when this fixture asks for one */
static const char *prompts_key;
static const char *prompts_path = "prompts/from-the-prompts.zs";
int hos_prompts_romfs(const char *key, const char *mod, char *out, size_t cap) {
    if (prompts_key && !strcmp(key, prompts_key) && !mod) {
        snprintf(out, cap, "%s", prompts_path);
        return 1;
    }
    return 0;
}

void hos_mods_init(const char *image_path);
int hos_mods_romfs(const char *rel, char *out, size_t cap);
int hos_mods_file(const char *rel, char *out, size_t cap);
int hos_mods_dir_extras(const char *rel, const char *base_dir, void (*fn)(void *, const char *, int, uint64_t),
                        void *arg);

static int failures;
#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); failures++; } } while (0)

static int has(const char *path, const char *part) { return strstr(path, part) != NULL; }

static int file_is(const char *rel, const char *part) {
    char out[4096] = "";
    return hos_mods_file(rel, out, sizeof out) && has(out, part);
}

static int romfs_is(const char *rel, const char *part) {
    char out[4096] = "";
    return hos_mods_romfs(rel, out, sizeof out) && has(out, part);
}

typedef struct { char name[8][256]; int n; } Listed;
static void listed(void *arg, const char *name, int is_dir, uint64_t size) {
    Listed *l = (Listed *)arg;
    (void)is_dir;
    (void)size;
    if (l->n < 8) snprintf(l->name[l->n], sizeof l->name[0], "%s", name);
    l->n++;
}
static int lists(const Listed *l, const char *name) {
    for (int i = 0; i < l->n && i < 8; i++)
        if (!strcmp(l->name[i], name)) return 1;
    return 0;
}

int main(void) {
    const char *root = getenv("HOS_MODS");
    if (!root) return 2;
    hos_mods_init(NULL);

    /* the exact table name the game opens is the one served, whichever other versions a mod carries */
    CHECK(file_is("System/Resource/ResourceSizeTable.Product.111.Nin_Ounce_NVN2.rsizetable.zs", "02-b"));
    CHECK(file_is("System/Resource/ResourceSizeTable.Product.110.Nin_Ounce_NVN2.rsizetable.zs", "01-a"));
    CHECK(!hos_mods_file("System/Resource/ResourceSizeTable.Product.112.Nin_Ounce_NVN2.rsizetable.zs", NULL, 0));
    /* keys are case- and separator-insensitive, as the normal RomFS open path is */
    CHECK(file_is("system\\resource\\resourcesizetable.product.111.nin_ounce_nvn2.rsizetable.zs", "02-b"));
    /* a folder is not a file */
    CHECK(!hos_mods_file("UI", NULL, 0));

    /* two mods with the same file: the one whose name sorts last wins; a file only one has is still served */
    CHECK(romfs_is("UI/Shared.zs", "02-b"));
    CHECK(romfs_is("UI/OnlyA.zs", "01-a"));
    CHECK(romfs_is("UI/ModOnly.zs", "02-b"));
    /* the deeper emulator layout (contents/<title id>/<name>/romfs) is a mod too */
    CHECK(romfs_is("Deep/File.zs", "0100000000001000"));
    CHECK(romfs_is("Deep/File.zs", "deep"));
    /* a folder whose name starts with '-' or '_' is off, and a folder with no romfs/exefs is not a mod */
    CHECK(!hos_mods_romfs("Off.zs", NULL, 0));
    CHECK(!hos_mods_romfs("Off2.zs", NULL, 0));
    CHECK(!hos_mods_romfs("NotAMod.zs", NULL, 0));
    /* a path only the prompts have is served by the prompts */
    prompts_key = "ui/promptsonly.zs";
    CHECK(romfs_is("UI/PromptsOnly.zs", "prompts/from-the-prompts.zs"));
    prompts_key = NULL;
    /* a folder's listing gets the mod-only entries, not the game's own (base_dir is the game's UI folder) */
    char base[4096];
    snprintf(base, sizeof base, "%s/game/UI", root);
    Listed l;
    memset(&l, 0, sizeof l);
    const int n = hos_mods_dir_extras("UI", base, listed, &l);
    CHECK(n == 3);   /* (OnlyA.zs, ModOnly.zs and the UI/Sub folder) */
    CHECK(lists(&l, "OnlyA.zs"));
    CHECK(lists(&l, "ModOnly.zs"));
    CHECK(lists(&l, "Sub"));
    CHECK(!lists(&l, "Shared.zs"));   /* (the game's own file is not an extra) */

    if (!failures) puts("ok: mod folders, last-sorted replacement, exact size-table lookup, prompts precedence");
    return failures ? 1 : 0;
}
