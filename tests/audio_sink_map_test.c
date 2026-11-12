/* The final mix's device-sink channel map, cut out of host/nn_audio.c: six speakers go out as FL FR FC LFE RL RR,
   and on a stereo device the centre, LFE and backs are folded down. Authored samples only; no game or device. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define FRAME 240
#define MAX_BUF 24
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

typedef struct {
    int used, buffers;
    float volume;
    float buf[MAX_BUF][FRAME];
} Mix;
static Mix mixes[1];
static int sink_mix = 0, sink_n = 6;
static int8_t sink_in[8];
static void effects_frame(int mix) { (void)mix; }

#include "audio_sink_map.inc"

static void near(float actual, float expected) {
    CHECK(fabsf(actual - expected) < 1e-5f);
}
static void source(void) {
    memset(mixes, 0, sizeof mixes);
    mixes[0].used = 1, mixes[0].buffers = 8, mixes[0].volume = 1.0f;
    for (int k = 0; k < 8; k++)
        for (int i = 0; i < FRAME; i++) mixes[0].buf[k][i] = (float)(k + 1);
    for (int k = 0; k < 8; k++) sink_in[k] = (int8_t)k;
    sink_mix = 0, sink_n = 6;
}
int main(void) {
    float out[6][FRAME];

    /* 5.1: one sink input per speaker, in the device's order, with nothing folded across */
    source();
    sink_map(out, 6);
    for (int k = 0; k < 6; k++) {
        near(out[k][0], (float)(k + 1));
        near(out[k][FRAME - 1], (float)(k + 1));
        for (int j = 0; j < 6; j++) if (j != k) near(out[j][0], (float)(j + 1));
    }

    /* stereo: FL and FR direct, FC and RL/RR at -3 dB, LFE at -6 dB, each on its own side */
    source();
    sink_map(out, 2);
    const float l = 1.0f + 3.0f * 0.7071f + 4.0f * 0.5f + 5.0f * 0.7071f;
    const float r = 2.0f + 3.0f * 0.7071f + 4.0f * 0.5f + 6.0f * 0.7071f;
    near(out[0][0], l);
    near(out[1][0], r);
    near(out[0][FRAME - 1], l);
    near(out[1][FRAME - 1], r);
    for (int k = 2; k < 6; k++) near(out[k][0], 0.0f);
    /* (FL never reaches the right speaker, FR never the left, and the centre and LFE reach both alike) */
    near(out[1][0] - out[0][0], 1.7071f);

    /* a four-channel device is a stereo layout here: the same fold-down */
    source();
    sink_map(out, 4);
    near(out[0][0], l);
    near(out[1][0], r);

    /* the final mix's volume scales the fold-down once */
    source();
    mixes[0].volume = 0.5f;
    sink_map(out, 2);
    near(out[0][0], l * 0.5f);
    near(out[1][0], r * 0.5f);

    /* a 7.1 sink names eight inputs, but the game's sink is six: the two side speakers are not folded in */
    source();
    sink_n = 8;
    sink_map(out, 6);
    for (int k = 0; k < 6; k++) near(out[k][0], (float)(k + 1));
    sink_map(out, 2);
    near(out[0][0], l);
    near(out[1][0], r);

    /* an unmapped or out-of-range input is silent, not read past the mix's buffers */
    source();
    sink_in[0] = -1, sink_in[1] = 8;
    sink_map(out, 6);
    near(out[0][0], 0.0f);
    near(out[1][0], 0.0f);
    near(out[2][0], 3.0f);

    /* no final mix: silence */
    sink_mix = -1;
    sink_map(out, 6);
    for (int k = 0; k < 6; k++) near(out[k][0], 0.0f);

    puts("PASS: 5.1 maps FL FR FC LFE RL RR and folds down to stereo; 7.1's extra inputs stay out");
    return 0;
}
