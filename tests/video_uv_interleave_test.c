/* video_uv_interleave (host/video_uv.h): planar U/V rows to NV12's interleaved UV,
   scalar and SSE2 paths, including the 16-pair boundary and tails. No game or GPU. */
#include "../host/video_uv.h"

#include <stdio.h>

static int check(int pairs) {
    uint8_t u[64], v[64], uv[128];
    for (int i = 0; i < pairs; i++) {
        u[i] = (uint8_t)(i * 3 + 1);
        v[i] = (uint8_t)(255 - i * 5);
        uv[2 * i] = uv[2 * i + 1] = 0;
    }
    video_uv_interleave(uv, u, v, pairs);
    for (int i = 0; i < pairs; i++) {
        if (uv[2 * i] != u[i] || uv[2 * i + 1] != v[i]) {
            printf("FAIL: %d pairs, pair %d: %u,%u expected %u,%u\n", pairs, i, uv[2 * i], uv[2 * i + 1], u[i], v[i]);
            return 1;
        }
    }
    return 0;
}

int main(void) {
    for (int pairs = 0; pairs <= 37; pairs++) {
        if (check(pairs)) return 1;
    }
    printf("video_uv_interleave: ok\n");
    return 0;
}
