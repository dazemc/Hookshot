pub const VSET_HEADER: &str = r#"
/* the four flags in one store (pack_flags in a64_lift.py): n, z, c, v are adjacent bytes of the context */
static inline __attribute__((always_inline)) void a64_set_nzcv(Ctx *c, uint32_t n, uint32_t z, uint32_t cf, uint32_t v) {
    const uint32_t w = n | z << 8 | cf << 16 | v << 24;
    memcpy(&c->n, &w, 4);
}
/* ---- whole-register vector writes (widen_vstores in a64_lift.py): one 16-byte store, kept whole (A64_OPAQUE hides
   the value from the compiler's store narrowing) ---- */
#if defined(__x86_64__) || defined(_M_X64)
#define A64_OPAQUE(x) __asm__("" : "+x"(x))
#else
#define A64_OPAQUE(x) ((void)0)
#endif
#if RECOMP_FPX_HOST == 2
static inline __attribute__((always_inline)) void a64_vst16(uint64_t *d, const void *s) {
    __m128i x = _mm_loadu_si128((const __m128i *)s);
    A64_OPAQUE(x);
    _mm_storeu_si128((__m128i *)d, x);
}
static inline __attribute__((always_inline)) void a64_vset64(uint64_t *d, uint64_t v) {
    __m128i x = _mm_set_epi64x(0, (long long)v);
    A64_OPAQUE(x);
    _mm_storeu_si128((__m128i *)d, x);
}
static inline __attribute__((always_inline)) void a64_vset128(uint64_t *d, uint64_t lo, uint64_t hi) {
    __m128i x = _mm_set_epi64x((long long)hi, (long long)lo);
    A64_OPAQUE(x);
    _mm_storeu_si128((__m128i *)d, x);
}
static inline __attribute__((always_inline)) void a64_vst_lo64(uint64_t *d, __m128i r) {   /* the low half, top zeroed */
    r = _mm_move_epi64(r);
    A64_OPAQUE(r);
    _mm_storeu_si128((__m128i *)d, r);
}
#else
static inline void a64_vst16(uint64_t *d, const void *s) { memcpy(d, s, 16); }
static inline void a64_vset64(uint64_t *d, uint64_t v) { d[0] = v; d[1] = 0; }
static inline void a64_vset128(uint64_t *d, uint64_t lo, uint64_t hi) { d[0] = lo; d[1] = hi; }
#endif
static inline __attribute__((always_inline)) void a64_vset_lo(uint64_t *d, const void *s, int n) {
    uint64_t v = 0;
    memcpy(&v, s, n);
    a64_vset64(d, v);
}
"#;

pub const VFCMP_HEADER: &str = r#"
/* ---- vector float compares on SSE (vectorize_fcmp): 1 when done (no NaN lane; under FZ no subnormal lane) ---- */
static const uint64_t a64_vzero[2] = {0, 0};   /* (the operand of a compare against zero) */
/* scalar FCMP / FCMPE: the flags from one native compare; 0 for the exact block. (Byte stores, as everywhere: one
   packed store of the four was slower, the compiler no longer forwarded single flags from it.) */
static inline __attribute__((always_inline)) int a64_fcmp_set(Ctx *c, uint32_t lt, uint32_t eq) {
    c->n = (uint8_t)lt;   /* lt: N, eq: Z, else C; V clear */
    c->z = (uint8_t)eq;
    c->c = (uint8_t)!lt;
    c->v = 0;
    return 1;
}
static inline __attribute__((always_inline)) int a64_fcmp32(Ctx *c, const void *pa, const void *pb) {
    uint32_t a, b;
    float fa, fb;
    memcpy(&a, pa, 4);
    memcpy(&b, pb, 4);
    memcpy(&fa, &a, 4);
    memcpy(&fb, &b, 4);
    if (__builtin_isunordered(fa, fb)) return 0;
    if ((c->fpcr & (1ULL << 24)) && ((a & 0x7fffffffu) - 1u < 0x7fffffu || (b & 0x7fffffffu) - 1u < 0x7fffffu)) return 0;
    return a64_fcmp_set(c, fa < fb, fa == fb);
}
static inline __attribute__((always_inline)) int a64_fcmp64(Ctx *c, const void *pa, const void *pb) {
    uint64_t a, b;
    double fa, fb;
    memcpy(&a, pa, 8);
    memcpy(&b, pb, 8);
    memcpy(&fa, &a, 8);
    memcpy(&fb, &b, 8);
    if (__builtin_isunordered(fa, fb)) return 0;
    if ((c->fpcr & (1ULL << 24)) && ((a & 0x7fffffffffffffffULL) - 1u < 0xfffffffffffffULL ||
                                     (b & 0x7fffffffffffffffULL) - 1u < 0xfffffffffffffULL)) return 0;
    return a64_fcmp_set(c, fa < fb, fa == fb);
}
static inline __attribute__((always_inline)) int a64_vfcmp(Ctx *c, uint64_t *d, const uint64_t *n, const uint64_t *m, int k, int op) {
#if RECOMP_FPX_HOST == 2
    const __m128i a = _mm_loadu_si128((const __m128i *)n), b = _mm_loadu_si128((const __m128i *)m);
    const __m128i abs_mask = _mm_set1_epi32(0x7fffffff), inf = _mm_set1_epi32(0x7f800000);
    const __m128i abs_a = _mm_and_si128(a, abs_mask), abs_b = _mm_and_si128(b, abs_mask);
    __m128i bad = _mm_or_si128(_mm_cmpgt_epi32(abs_a, inf), _mm_cmpgt_epi32(abs_b, inf));   /* NaN */
    if (c->fpcr & (1ULL << 24)) {   /* FZ: a subnormal is flushed (and flagged) by the exact block */
        const __m128i z = _mm_setzero_si128();
        const __m128i sub_a = _mm_andnot_si128(_mm_cmpeq_epi32(_mm_and_si128(a, inf), inf), _mm_and_si128(_mm_cmpeq_epi32(_mm_and_si128(a, inf), z), _mm_cmpgt_epi32(abs_a, z)));
        const __m128i sub_b = _mm_andnot_si128(_mm_cmpeq_epi32(_mm_and_si128(b, inf), inf), _mm_and_si128(_mm_cmpeq_epi32(_mm_and_si128(b, inf), z), _mm_cmpgt_epi32(abs_b, z)));
        bad = _mm_or_si128(bad, _mm_or_si128(sub_a, sub_b));
    }
    if (_mm_movemask_epi8(bad) & (k == 4 ? 0xFFFF : 0x00FF)) return 0;
    const __m128 fa = _mm_castsi128_ps(a), fb = _mm_castsi128_ps(b);
    const __m128i r = _mm_castps_si128(op == 0 ? _mm_cmpge_ps(fa, fb) : op == 1 ? _mm_cmpgt_ps(fa, fb) : _mm_cmpeq_ps(fa, fb));
    if (k == 4) _mm_storeu_si128((__m128i *)d, r); else a64_vst_lo64(d, r);
    return 1;
#else
    (void)c; (void)d; (void)n; (void)m; (void)k; (void)op;
    return 0;
#endif
}
"#;

#[allow(dead_code)]
pub const VFPX_HEADER: &str = r#"
/* ---- 2/4-lane single-precision ops on SSE (vectorize_fpx in a64_lift.py): a lane kept only where the scalar fast
   path would keep it (the same tests, on every lane); otherwise each lane goes through its scalar helper ---- */
#if RECOMP_FPX_HOST == 2
static inline __m128i a64_v_ult(__m128i x, uint32_t k) {   /* unsigned x < k, per lane */
    return _mm_cmplt_epi32(_mm_xor_si128(x, _mm_set1_epi32((int)0x80000000u)), _mm_set1_epi32((int)(k ^ 0x80000000u)));
}
static inline __m128i a64_v_abs(__m128i x) { return _mm_and_si128(x, _mm_set1_epi32(0x7fffffff)); }
static inline __m128i a64_v_big(__m128i t) { return a64_v_ult(_mm_sub_epi32(a64_v_abs(t), _mm_set1_epi32(0x00800001)), 0x7effffffu); }
static inline __m128i a64_v_zero(__m128i x) { return _mm_cmpeq_epi32(a64_v_abs(x), _mm_setzero_si128()); }
static inline __m128i a64_v_nonfin(__m128i t) { const __m128i e = _mm_set1_epi32(0x7f800000); return _mm_cmpeq_epi32(_mm_and_si128(t, e), e); }
static inline __m128i a64_v_subn(__m128i x) { return a64_v_ult(_mm_sub_epi32(_mm_slli_epi32(x, 1), _mm_set1_epi32(1)), 0x00ffffffu); }
/* lanes that overflowed (an infinite result of finite operands), kept once FPSR.OFC is raised (FPX_FZ_HEADER) */
static inline __m128i a64_v_ovf(const Ctx *c, __m128i a, __m128i b, __m128i t) {
    if (!(c->fpsr & 4u)) return _mm_setzero_si128();
    const __m128i inf = _mm_cmpeq_epi32(a64_v_abs(t), _mm_set1_epi32(0x7f800000));
    return _mm_andnot_si128(_mm_or_si128(a64_v_nonfin(a), a64_v_nonfin(b)), inf);
}
#define A64_VFPX_SUM(name, op, scalar) \
static __attribute__((noinline, cold)) void a64_vfpx_##name##_lanes(Ctx *c, uint64_t *d, const uint64_t *n, const uint64_t *m, int k) { \
    uint32_t a[4], b[4], r[4] = {0}; memcpy(a, n, 16); memcpy(b, m, 16); \
    for (int j = 0; j < k; j++) r[j] = (uint32_t)scalar(c, a[j], b[j]); \
    memcpy(d, r, 16); } \
static inline __attribute__((always_inline)) void a64_vfpx_##name(Ctx *c, uint64_t *d, const uint64_t *n, const uint64_t *m, int k) { \
    if (RECOMP_LIKELY(RECOMP_FPX_OPEN(c))) { \
        const __m128i a = _mm_loadu_si128((const __m128i *)n), b = _mm_loadu_si128((const __m128i *)m); \
        const __m128i t = _mm_castps_si128(op(_mm_castsi128_ps(a), _mm_castsi128_ps(b))); \
        __m128i bad = a64_v_nonfin(t), r = t; \
        if (RECOMP_FPX_FZ(c)) { \
            bad = _mm_or_si128(bad, _mm_or_si128(a64_v_subn(a), a64_v_subn(b))); \
            if (c->fpsr & 8u) { /* (flags raised: a sum below the normal range, exact there, is a zero of its sign) */ \
                const __m128i tiny = _mm_cmplt_epi32(a64_v_abs(t), _mm_set1_epi32(0x00800000)); \
                r = _mm_or_si128(_mm_andnot_si128(tiny, t), _mm_and_si128(tiny, _mm_and_si128(t, _mm_set1_epi32((int)0x80000000u)))); \
            } else { \
                bad = _mm_or_si128(bad, _mm_andnot_si128(_mm_or_si128(a64_v_big(t), a64_v_zero(t)), _mm_set1_epi32(-1))); \
            } \
        } \
        const int lanes = k == 4 ? 0xFFFF : 0x00FF; /* (overflowed lanes looked at only once a lane is out) */ \
        if (RECOMP_LIKELY(!(_mm_movemask_epi8(bad) & lanes)) || \
            !(_mm_movemask_epi8(_mm_andnot_si128(a64_v_ovf(c, a, b, t), bad)) & lanes)) { \
            if (k == 4) _mm_storeu_si128((__m128i *)d, r); else a64_vst_lo64(d, r); \
            return; } } \
    a64_vfpx_##name##_lanes(c, d, n, m, k); }
#else
#define A64_VFPX_SUM(name, op, scalar) \
static __attribute__((noinline)) void a64_vfpx_##name(Ctx *c, uint64_t *d, const uint64_t *n, const uint64_t *m, int k) { \
    uint32_t a[4], b[4], r[4] = {0}; memcpy(a, n, 16); memcpy(b, m, 16); \
    for (int j = 0; j < k; j++) r[j] = (uint32_t)scalar(c, a[j], b[j]); \
    memcpy(d, r, 16); }
#endif
"#;

#[allow(dead_code)]
pub const VFPX_MUL_HEADER: &str = r#"
static __attribute__((noinline, cold)) void a64_vfpx_mul32_lanes(Ctx *c, uint64_t *d, const uint64_t *n, const uint64_t *m) {
    uint32_t a[4], b[4], r[4]; memcpy(a, n, 16); memcpy(b, m, 16);
    for (int j = 0; j < 4; j++) r[j] = (uint32_t)a64_fpx_mul32_fma32(c, a[j], b[j], (a[j] ^ b[j]) & 0x80000000u);
    memcpy(d, r, 16); }
static inline __attribute__((always_inline)) void a64_vfpx_mul32(Ctx *c, uint64_t *d, const uint64_t *n, const uint64_t *m, int k) {
    (void)k;
#if RECOMP_FPX_HOST == 2
    if (RECOMP_LIKELY(RECOMP_FPX_OPEN(c))) {
        const __m128i a = _mm_loadu_si128((const __m128i *)n), b = _mm_loadu_si128((const __m128i *)m);
        const __m128i t = _mm_castps_si128(_mm_mul_ps(_mm_castsi128_ps(a), _mm_castsi128_ps(b)));
        /* kept: big, or a finite product of a zero (RECOMP_FPX_KEEP32); under FZ no subnormal operand */
        const __m128i keep = _mm_or_si128(a64_v_big(t), _mm_andnot_si128(a64_v_nonfin(t), _mm_or_si128(a64_v_zero(a), a64_v_zero(b))));
        __m128i bad = _mm_andnot_si128(keep, _mm_set1_epi32(-1)), r = t;
        if (RECOMP_FPX_FZ(c)) {
            bad = _mm_or_si128(bad, _mm_or_si128(a64_v_subn(a), a64_v_subn(b)));
            if (c->fpsr & 8u) {   /* (flags raised: a tiny product is a zero of its sign; the smallest normal: exact) */
                const __m128i abs_t = a64_v_abs(t), minn = _mm_set1_epi32(0x00800000);
                const __m128i tiny = _mm_cmplt_epi32(abs_t, minn);
                const __m128i fin = _mm_andnot_si128(a64_v_nonfin(t), _mm_set1_epi32(-1));
                bad = _mm_andnot_si128(_mm_and_si128(tiny, fin), bad);   /* (still bad if a subnormal operand) */
                bad = _mm_or_si128(bad, _mm_or_si128(a64_v_subn(a), a64_v_subn(b)));
                bad = _mm_or_si128(bad, _mm_cmpeq_epi32(abs_t, minn));
                r = _mm_or_si128(_mm_andnot_si128(tiny, t), _mm_and_si128(tiny, _mm_and_si128(t, _mm_set1_epi32((int)0x80000000u))));
            }
        }
        if (RECOMP_LIKELY(!_mm_movemask_epi8(bad)) || !_mm_movemask_epi8(_mm_andnot_si128(a64_v_ovf(c, a, b, t), bad))) {
            _mm_storeu_si128((__m128i *)d, r);
            return;
        }
    }
#endif
    a64_vfpx_mul32_lanes(c, d, n, m);
}
"#;
