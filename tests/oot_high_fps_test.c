/* The 60 Hz sound job gate and the rounded-up solver steps above a 60 fps cap, from host/oot_high_fps.h.
   Pure arithmetic: no game, renderer or audio device. */
#include "oot_high_fps.h"
#include <math.h>
#include <stdint.h>
#include <stdio.h>

static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)
static uint32_t bits(float f) { return oot_float_bits(f); }
static int runs(float step, int frames, int timed) {
    OotSoundGate g = OOT_SOUND_GATE_INIT;
    int n = 0;
    for (int i = 0; i < frames; i++) {
        const int run = oot_sound_gate(&g, 0, step, timed);
        CHECK(oot_sound_gate(&g, 1, step, timed) == run);   /* (phase 1 follows phase 0) */
        n += run;
    }
    return n;
}
/* (the sound engine's time over these frames' steps against the real time: what it is handed each frame the job runs) */
static double sound_clock(const float *steps, int n) {
    OotSoundGate g = OOT_SOUND_GATE_INIT;
    double real = 0, sound = 0;
    for (int i = 0; i < n; i++) {
        real += steps[i];
        if (oot_sound_gate(&g, 0, steps[i], 1)) sound += oot_bits_float(oot_sound_step(oot_float_bits(steps[i]), 1));
    }
    return sound / real;
}
int main(void) {
    CHECK(runs(0.5f, 120, 1) == 60);
    const int n144 = runs(60.0f / 144.0f, 144, 1);
    CHECK(n144 == 60 || n144 == 59);
    CHECK(runs(60.0f / 1000.0f, 1000, 1) >= 59 && runs(60.0f / 1000.0f, 1000, 1) <= 60);
    CHECK(runs(0.5f, 120, 0) == 120);   /* (a cap of 60: every frame) */
    CHECK(runs(1.0f, 60, 1) == 60 && runs(1.5f, 60, 1) == 60);
    CHECK(runs(0.0f, 60, 1) == 60 && runs(-1.0f, 60, 1) == 60 && runs(NAN, 60, 1) == 60);
    {   /* (a whole step after partial ones runs with the game's step and keeps the remainder) */
        OotSoundGate g = OOT_SOUND_GATE_INIT;
        CHECK(oot_sound_gate(&g, 0, 0.75f, 1) == 0 && oot_sound_gate(&g, 2, 0.75f, 1) == 0);
        CHECK(oot_sound_gate(&g, 0, 1.0f, 1) == 1 && g.part == 0.75f && oot_sound_gate(&g, 1, 1.0f, 1) == 1);
        CHECK(oot_sound_gate(&g, 0, 0.75f, 1) == 1 && g.part == 0.5f);
        CHECK(oot_sound_gate(&g, 0, 0.0f, 1) == 1 && g.part == 0.0f);   /* (a step of 0: as the game has it, the remainder cleared) */
    }
    {   /* (around 60 fps the sound clock keeps the real one: dropping the remainder at each whole step made it 0.51 of it) */
        static float steps[20000];
        for (int i = 0; i < 20000; i++) steps[i] = i & 1 ? 1.02f : 0.98f;
        double r = sound_clock(steps, 20000);
        CHECK(r > 0.999 && r < 1.001);
        for (int i = 0; i < 20000; i++) steps[i] = i & 1 ? 1.1f : 0.9f;
        r = sound_clock(steps, 20000);
        CHECK(r > 0.999 && r < 1.001);
        uint32_t x = 12345;   /* (mean 1, about 0.03 apart: a sum of uniforms) */
        for (int i = 0; i < 20000; i++) {
            float u = 0;
            for (int k = 0; k < 12; k++) x = x * 1664525u + 1013904223u, u += (float)(x >> 8) / 16777216.0f;
            steps[i] = 1.0f + (u - 6.0f) * 0.03f;
        }
        r = sound_clock(steps, 20000);
        CHECK(r > 0.999 && r < 1.001);
        for (int i = 0; i < 20000; i++) steps[i] = 0.5f;
        r = sound_clock(steps, 20000);
        CHECK(r > 0.999 && r < 1.001);
    }
    {   /* (a cap of 60 runs every phase: before any phase 0, and after a phase 0 that held the job at a cap above 60) */
        OotSoundGate g = OOT_SOUND_GATE_INIT;
        CHECK(oot_sound_gate(&g, 1, 1.0f, 0) == 1 && oot_sound_gate(&g, 2, 1.0f, 0) == 1);
        OotSoundGate h = OOT_SOUND_GATE_INIT;
        CHECK(oot_sound_gate(&h, 1, 1.0f, 1) == 1);   /* (above 60, before the first phase 0: it runs) */
        CHECK(oot_sound_gate(&h, 0, 0.5f, 1) == 0 && oot_sound_gate(&h, 1, 0.5f, 0) == 1 && h.part == 0.0f);   /* (the cap changed) */
        CHECK(oot_sound_gate(&h, 2, 0.5f, 0) == 1 && oot_sound_gate(&h, 0, 0.5f, 0) == 1);
    }
    CHECK(oot_sound_step(bits(0.5f), 1) == bits(1.0f));
    CHECK(oot_sound_step(bits(0.5f), 0) == bits(0.5f));
    CHECK(oot_sound_step(bits(1.25f), 1) == bits(1.25f) && oot_sound_step(bits(0.0f), 1) == bits(0.0f));
    /* (the game's: (int)(step * n + 0.001), n the steps of a 60 fps frame) */
    #define GAME(step, n) ((int32_t)((step) * (n) + 0.001f))
    #define OURS(step, n, timed) oot_solver_steps(GAME(step, n), bits((step) * (n) + 0.001f), bits(n), timed)
    CHECK(OURS(1.0f, 4.0f, 1) == 4 && OURS(1.0f, 8.0f, 1) == 8 && OURS(1.0f, 8.0f, 0) == 8);
    CHECK(OURS(0.5f, 4.0f, 1) == 2 && OURS(0.49f, 4.0f, 1) == 2 && OURS(0.52f, 4.0f, 1) == 2 && GAME(0.49f, 4.0f) == 1);
    CHECK(OURS(0.49f, 8.0f, 1) == 4 && OURS(0.5f, 8.0f, 1) == 4 && GAME(0.49f, 8.0f) == 3);
    CHECK(OURS(0.25f, 4.0f, 1) == 1 && OURS(0.24f, 4.0f, 1) == 1 && OURS(0.06f, 4.0f, 1) == 1 && GAME(0.24f, 4.0f) == 0);
    CHECK(OURS(60.0f / 144.0f, 4.0f, 1) == 2 && OURS(60.0f / 90.0f, 4.0f, 1) == 3);
    CHECK(OURS(0.49f, 4.0f, 0) == 1 && OURS(0.06f, 4.0f, 0) == 0);   /* (a cap of 60: the game's count) */
    CHECK(OURS(1.6f, 4.0f, 1) == 6 && OURS(1.708f, 8.0f, 1) == 13);   /* (a slow frame: the game's count) */
    CHECK(oot_solver_steps(3, bits(NAN), bits(4.0f), 1) == 3 && oot_solver_steps(0, bits(-1.0f), bits(4.0f), 1) == 0);
    CHECK(oot_solver_steps(1, bits(2.0f), bits(0.0f), 1) == 1);   /* (no steps a frame: the game's) */
    if (!fails) puts("PASS: sound at 60 Hz and solver steps rounded up above 60 fps, unchanged at 60");
    return fails != 0;
}
