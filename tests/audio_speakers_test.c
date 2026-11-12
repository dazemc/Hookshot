/* The speakers the game is told about (host/nn_audio.c au_speakers): Auto follows the default output's channels,
   Stereo and 5.1 come from the Insert menu's audio_output= line, and HOS_AUDIO_CHANNELS wins over both. No device. */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef unsigned long DWORD;
#define MAX_PATH 260
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

static const char *env_channels;
static int device_channels, device_asked;
static char logged[512];

static DWORD hos_game_path_a(char *path, DWORD size) {
    const int n = snprintf(path, size, "./hookshot.exe");
    return n > 0 ? (DWORD)n : 0;
}
static void au_lock(void) {}
static void au_unlock(void) {}
static int au_device_channels(void) { device_asked++; return device_channels; }
static void hos_log(const char *format, ...) {
    va_list args;
    va_start(args, format);
    vsnprintf(logged, sizeof logged, format, args);
    va_end(args);
}
static const char *fixture_getenv(const char *name) {
    return !strcmp(name, "HOS_AUDIO_CHANNELS") ? env_channels : NULL;
}
#define getenv fixture_getenv

#include "audio_speakers.inc"

int main(int argc, char **argv) {
    CHECK(argc == 7);
    const char *saved = argv[1];
    device_channels = atoi(argv[2]);
    const int want = atoi(argv[3]), want_asked = atoi(argv[4]);
    env_channels = strcmp(argv[5], "-") ? argv[5] : NULL;
    if (strcmp(saved, "-")) {   /* (the line as the menu's save writes it) */
        FILE *f = fopen("hookshot_options.ini", "w");
        CHECK(f);
        fprintf(f, "output_res=1\naudio_output=%s\nmicrophone=1\noptions_version=3\n", saved);
        fclose(f);
    }
    const int n = au_speakers();
    CHECK(au_speakers() == n);   /* (asked once: the second answer is the first) */
    if (n != want || device_asked != want_asked || !strstr(logged, argv[6])) {
        fprintf(stderr, "FAIL: saved %s, device %d, env %s: %d speakers (want %d), device asked %d (want %d), log \"%s\"\n",
                saved, device_channels, argv[5], n, want, device_asked, want_asked, logged);
        return 1;
    }
    puts("PASS: the speakers follow the menu, the default output and HOS_AUDIO_CHANNELS");
    return 0;
}
