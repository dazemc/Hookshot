/* host/dav1d_av1.c against tests/dav1d_av1/lossless_64x48.ivf: every picture of a synthetic lossless AV1 stream comes out,
   in order, with its pts, as the exact NV12 of what was encoded; the end drains what dav1d holds back; after a flush the
   same stream decodes again. The clip is 12 pictures of a formula pattern, 64x48, encoded losslessly with libaom, so no
   game data. Built by builder/rust/tests/host_video_fixtures.rs and, for the Windows library, by windows-builder.yml.

   cc -std=c11 -I<dav1d>/include tests/dav1d_av1_test.c host/dav1d_av1.c <dav1d>/lib/libdav1d.a -lpthread -lm
   HOS_DAV1D_THREADS=1 ./a.out tests/dav1d_av1/lossless_64x48.ivf */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void *dav1d_av1_open(int w, int h);
void dav1d_av1_decode(void *m, const uint8_t *data, size_t n, int64_t pts,
                      void (*fn)(void *, const uint8_t *, const uint8_t *, int, int, int, int64_t), void *user);
void dav1d_av1_flush(void *m);
void dav1d_av1_close(void *m);

void hos_log(const char *fmt, ...) {
    va_list a;
    va_start(a, fmt);
    vfprintf(stderr, fmt, a);
    va_end(a);
    fputc('\n', stderr);
}
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

enum { W = 64, H = 48, N = 12 };
static uint8_t expect(int f, int plane, int x, int y) {
    return plane == 0 ? (uint8_t)(x * 3 + y * 2 + f * 5)
                      : plane == 1 ? (uint8_t)(64 + x * 2 + f * 3) : (uint8_t)(192 - y * 3 + f * 2);
}
typedef struct { int n; int64_t pts[64]; } Seen;
static void picture(void *user, const uint8_t *y, const uint8_t *uv, int w, int h, int stride, int64_t pts) {
    Seen *s = (Seen *)user;
    CHECK(w == W && h == H && stride >= W && s->n < N);
    const int f = s->n;
    for (int r = 0; r < H; r++)
        for (int x = 0; x < W; x++) CHECK(y[r * stride + x] == expect(f, 0, x, r));
    for (int r = 0; r < H / 2; r++)
        for (int x = 0; x < W / 2; x++) {
            CHECK(uv[r * stride + 2 * x] == expect(f, 1, x, r));
            CHECK(uv[r * stride + 2 * x + 1] == expect(f, 2, x, r));
        }
    s->pts[s->n++] = pts;
}
int main(int argc, char **argv) {
    CHECK(argc == 2);
    FILE *fp = fopen(argv[1], "rb");
    CHECK(fp);
    static uint8_t ivf[1 << 16];
    const size_t size = fread(ivf, 1, sizeof ivf, fp);
    fclose(fp);
    CHECK(size > 32 && !memcmp(ivf, "DKIF", 4));
    void *m = dav1d_av1_open(W, H);
    CHECK(m);
    for (int pass = 0; pass < 2; pass++) {   /* (the second after a flush: a movie started again) */
        Seen s = {0};
        int units = 0;
        for (size_t at = 32; at + 12 <= size;) {
            uint32_t n;
            int64_t pts;
            memcpy(&n, ivf + at, 4), memcpy(&pts, ivf + at + 4, 8);
            CHECK(at + 12 + n <= size);
            dav1d_av1_decode(m, ivf + at + 12, n, pts * 1000, picture, &s);
            at += 12 + n, units++;
        }
        CHECK(units == N);
        dav1d_av1_decode(m, NULL, 0, 0, picture, &s);   /* (the end) */
        CHECK(s.n == N);
        for (int i = 0; i < N; i++) CHECK(s.pts[i] == (int64_t)i * 1000);
        dav1d_av1_flush(m);
    }
    dav1d_av1_close(m);
    dav1d_av1_close(NULL);
    puts("ok");
    return 0;
}
