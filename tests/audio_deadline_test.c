/* The mixer's deadline from host/nn_audio.c: a wake under three seconds late mixes the owed frames on the old grid,
   a longer pause jumps to now. Authored times only; no game or device. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

#include "audio_deadline.inc"

/* frames mixed before the deadline is in the future again, with `now` held still */
static int caught_up(int64_t late) {
    const int64_t period = 5000000, start = 1000000000, now = start + late;
    int64_t next = start;
    int n = 0;
    while (next <= now && n < 10000) {
        next = audio_mix_deadline(now, next, period);
        n++;
    }
    return n;
}
int main(void) {
    const int64_t period = 5000000, start = 1000000000;
    /* 40 ms late: still on the old grid, nine frames (the last one lands on now) */
    CHECK(audio_mix_deadline(start + 40000000, start, period) == start + period);
    CHECK(caught_up(40000000) == 9);
    /* 150 ms late: the old 100 ms jump mixed one frame and left the sound behind */
    CHECK(caught_up(150000000) == 31);
    /* a four-second pause is not mixed out */
    CHECK(caught_up(4000000000LL) == 1);
    CHECK(audio_mix_deadline(start + 4000000000LL, start, period) == start + 4000000000LL + period);
    puts("PASS: a hitch is mixed out on the old grid, a long pause is not");
    return 0;
}
