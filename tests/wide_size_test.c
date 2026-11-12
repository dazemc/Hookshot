/* hos_wide_size.h: the 21:9 native picture and the 32:9 cap. No game or GPU. */
#include "../host/hos_wide_size.h"
#include <stdio.h>

static int check(double ratio, double max_px, unsigned want_w, unsigned want_h) {
    unsigned w = 0, h = 0;
    hos_wide_size(2160, ratio, max_px, &w, &h);
    if (w != want_w || h != want_h) return printf("FAIL: %.4f -> %ux%u, want %ux%u\n", ratio, w, h, want_w, want_h), 1;
    return 0;
}
int main(void) {
    if (check(5120.0 / 2160.0, 5120.0 * 2160.0, 5120, 2160)) return 1;
    if (check(32.0 / 9.0, 5120.0 * 2160.0, 6256, 1760)) return 1;
    if (check(32.0 / 9.0, 3840.0 * 2160.0, 5400, 1520)) return 1;
    printf("wide_size: ok\n");
    return 0;
}
