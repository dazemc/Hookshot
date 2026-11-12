/* The voice resampler in host/nn_audio.c's render_voice: sample rate x pitch, linear between source frames.
   Authored source frames only; no game, wave buffer or output device. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

#include "audio_resample_types.inc"

static Voice voices[MAX_VOICES];
static Mix mixes[MAX_MIXES];
static Splitter splitters[MAX_SPLITTERS];

static int next_source_frame(Voice *v);
static void biquad_frame(float *samples, const float b[3], const float a[2], float z[2]);

#include "audio_resample_render.inc"

static int source_index, source_constant;
static int next_source_frame(Voice *v) {
    /* frame n is 10*n, so a half-rate stream advances 5 an output frame and a double-rate one 20 */
    const float s = source_constant ? 1.0f : (float)(++source_index * 10);
    for (int c = 0; c < v->channels; c++) v->cur[c] = s + (float)c * 1000.0f;
    return 1;
}
static void biquad_frame(float *samples, const float b[3], const float a[2], float z[2]) {
    (void)samples, (void)b, (void)a, (void)z;
}
static void near_at(float actual, float expected, int line) {
    if (fabsf(actual - expected) >= 1e-4f) {
        fprintf(stderr, "FAIL:%d near(%g, %g)\n", line, (double)actual, (double)expected);
        exit(1);
    }
}
#define near(actual, expected) near_at((actual), (expected), __LINE__)
static void setup(int rate, float pitch, int channels) {
    memset(voices, 0, sizeof voices);
    memset(mixes, 0, sizeof mixes);
    memset(splitters, 0, sizeof splitters);
    source_index = 0, source_constant = 0;
    Voice *v = &voices[0];
    v->used = 1, v->state = PLAY_STARTED, v->channels = channels;
    v->rate = rate, v->pitch = pitch;
    v->volume = v->ramp_volume = 1.0f, v->snap = 0;
    v->dest_kind = DEST_MIX, v->dest = 0;
    for (int c = 0; c < channels; c++) v->vol[c][0] = v->ramp_vol[c][0] = 1.0f;
    mixes[0].used = 1, mixes[0].buffers = 1, mixes[0].volume = 1.0f;
}
int main(void) {
    /* 24 kHz: two output frames per source frame, linear between them */
    setup(24000, 1.0f, 1);
    render_voice(&voices[0]);
    CHECK(source_index == 120);
    near(mixes[0].buf[0][0], 0.0f);
    near(mixes[0].buf[0][1], 0.0f);
    near(mixes[0].buf[0][2], 5.0f);
    near(mixes[0].buf[0][3], 10.0f);
    near(mixes[0].buf[0][4], 15.0f);
    near(mixes[0].buf[0][FRAME - 1], (float)((FRAME - 2) / 2 * 10));
    CHECK(voices[0].frac == 0.0);

    /* 96 kHz: one output frame per two source frames, linearly held */
    setup(96000, 1.0f, 1);
    render_voice(&voices[0]);
    CHECK(source_index == 480);
    near(mixes[0].buf[0][0], 10.0f);
    near(mixes[0].buf[0][1], 30.0f);
    near(mixes[0].buf[0][2], 50.0f);

    /* pitch doubles and halves the step like the rate does */
    setup(RATE, 2.0f, 1);
    render_voice(&voices[0]);
    CHECK(source_index == 480);
    near(mixes[0].buf[0][0], 10.0f);
    near(mixes[0].buf[0][1], 30.0f);
    setup(RATE, 0.5f, 1);
    render_voice(&voices[0]);
    CHECK(source_index == 120);
    near(mixes[0].buf[0][2], 5.0f);

    /* a step of 1 runs the game's own frames: no interpolation, one source frame per output frame */
    setup(24000, 2.0f, 1);
    render_voice(&voices[0]);
    CHECK(source_index == 240);
    near(mixes[0].buf[0][0], 0.0f);
    near(mixes[0].buf[0][1], 10.0f);
    near(mixes[0].buf[0][2], 20.0f);

    /* stereo keeps its two channels on the same source frame, panned to their own mix buffers */
    setup(24000, 1.0f, 2);
    mixes[0].buffers = 2;
    voices[0].vol[0][0] = voices[0].ramp_vol[0][0] = 1.0f;
    voices[0].vol[1][0] = voices[0].ramp_vol[1][0] = 0.0f;
    voices[0].vol[1][1] = voices[0].ramp_vol[1][1] = 1.0f;
    render_voice(&voices[0]);
    near(mixes[0].buf[0][2], 5.0f);
    near(mixes[0].buf[1][2], 505.0f);

    /* 44.1 kHz keeps the fraction across the frame boundary instead of dropping it */
    setup(44100, 1.0f, 1);
    render_voice(&voices[0]);
    CHECK(source_index == 220);
    near(voices[0].frac, 0.5f);
    for (int i = 1; i < FRAME; i++) CHECK(mixes[0].buf[0][i] >= mixes[0].buf[0][i - 1]);

    /* a held volume ramps over one frame, and the last sample reaches the gain asked for */
    setup(RATE, 1.0f, 1);
    source_constant = 1;
    voices[0].ramp_volume = 0.0f, voices[0].volume = 1.0f;
    render_voice(&voices[0]);
    near(mixes[0].buf[0][0], 0.0f);   /* (a step of 1 outputs the previous source frame) */
    near(mixes[0].buf[0][1], 2.0f / FRAME);
    near(mixes[0].buf[0][FRAME - 1], 1.0f);

    puts("PASS: voice rate x pitch resamples linearly, keeps its fraction, and stereo keeps both channels");
    return 0;
}
