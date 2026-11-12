/* Planar 4:2:0 U/V rows to NV12's interleaved UV row: (u0, v0, u1, v1, ...).
   SSE2 on x86-64 interleaves 16 pairs at a time; the scalar tail is the same everywhere. */
#pragma once

#include <stdint.h>

#if defined(__x86_64__) || defined(_M_X64)
#include <emmintrin.h>
#endif

static inline void video_uv_interleave(uint8_t *uv, const uint8_t *u, const uint8_t *v, int pairs) {
    int x = 0;
#if defined(__x86_64__) || defined(_M_X64)
    for (; x + 16 <= pairs; x += 16) {
        const __m128i u16 = _mm_loadu_si128((const __m128i *)(u + x));
        const __m128i v16 = _mm_loadu_si128((const __m128i *)(v + x));
        _mm_storeu_si128((__m128i *)(uv + 2 * x), _mm_unpacklo_epi8(u16, v16));
        _mm_storeu_si128((__m128i *)(uv + 2 * x + 16), _mm_unpackhi_epi8(u16, v16));
    }
#endif
    for (; x < pairs; x++) {
        uv[2 * x] = u[x];
        uv[2 * x + 1] = v[x];
    }
}
