// The replay's reserved core (host/hos_thread.c pick_reserved_core and parse_cpu_list, upstream a435efd0 #2603 and
// 262f5614 #2805): the last core of the fastest kind with 4+ of them, the last core on a CPU of one kind, none under
// 8 logical processors, and the Linux hybrid cpulist. No game, no Windows: builder/rust/tests/reserved_core.rs cuts the
// two functions out of the host source and compiles this.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); exit(1); } } while (0)

/* PICK_RESERVED_CORE */
#if !defined(_WIN32)
/* PARSE_CPU_LIST */
#endif

struct Core { uint64_t mask; unsigned char cls; };

static uint64_t pick(const struct Core *cores, int n) {
    uint64_t masks[64];
    unsigned char classes[64];
    for (int i = 0; i < n; i++) masks[i] = cores[i].mask, classes[i] = cores[i].cls;
    return pick_reserved_core(masks, classes, n);
}

// n cores of `per` logical processors each from logical processor `first`, all of class cls
static void add(struct Core *v, int *at, int n, int per, int first, unsigned char cls) {
    for (int i = 0; i < n; i++) {
        v[*at].mask = ((1ull << per) - 1) << (first + i * per);
        v[*at].cls = cls;
        (*at)++;
    }
}

int main(void) {
    struct Core v[64];
    int n = 0;
    add(v, &n, 12, 2, 0, 0);
    CHECK(pick(v, n) == 0xC00000);   // Ryzen 9 3900X: 12 cores with SMT, one kind
    n = 0;
    add(v, &n, 6, 2, 0, 0);
    CHECK(pick(v, n) == 0xC00);      // Ryzen 5 7600X
    n = 0;
    add(v, &n, 8, 1, 0, 0);
    CHECK(pick(v, n) == 0x80);       // 8 cores without SMT
    n = 0;
    add(v, &n, 4, 2, 0, 0);
    CHECK(pick(v, n) == 0xC0);       // 4 cores with SMT: 8 logical processors, one kind
    n = 0;
    add(v, &n, 6, 1, 0, 0);
    CHECK(pick(v, n) == 0);          // 6 cores without SMT: fewer than 8 logical processors, none kept
    n = 0;
    add(v, &n, 8, 2, 0, 1);          // i7-12700K: 8 performance cores with HT (0-15), then 4 efficiency cores (16-19)
    add(v, &n, 4, 1, 16, 0);
    CHECK(pick(v, n) == 0xC000);
    n = 0;
    add(v, &n, 8, 2, 0, 1);          // i9-13900K: 8 performance cores with HT (0-15), then 16 efficiency cores (16-31)
    add(v, &n, 16, 1, 16, 0);
    CHECK(pick(v, n) == 0xC000);
    n = 0;
    add(v, &n, 4, 1, 0, 0);          // efficiency cores listed first, then performance cores: still the last fast one
    add(v, &n, 4, 2, 4, 1);
    CHECK(pick(v, n) == 0xC00);
    n = 0;
    add(v, &n, 4, 1, 0, 1);          // Lunar Lake: 4 performance cores, 4 efficiency cores, no HT: one of the 4 kept
    add(v, &n, 4, 1, 4, 0);
    CHECK(pick(v, n) == 0x8);
    n = 0;
    add(v, &n, 2, 2, 0, 1);          // i5-1235U: 2 performance cores with HT, 8 efficiency cores: neither of the 2 kept
    add(v, &n, 8, 1, 4, 0);
    CHECK(pick(v, n) == 0);
    n = 0;
    add(v, &n, 2, 2, 0, 1);          // i3-1215U: 2 performance cores with HT, 4 efficiency cores (8 logical): none kept
    add(v, &n, 4, 1, 4, 0);
    CHECK(pick(v, n) == 0);
    n = 0;
    add(v, &n, 3, 2, 0, 1);          // 3 fast cores of more than one kind: none kept
    add(v, &n, 8, 1, 6, 0);
    CHECK(pick(v, n) == 0);
    CHECK(pick(v, 0) == 0);          // empty
    n = 0;
    add(v, &n, 8, 2, 0, 0);
    v[n].mask = 0;                   // (an empty mask is no core, of whatever class)
    v[n].cls = 1;
    n++;
    CHECK(pick(v, n) == 0xC000);

#if !defined(_WIN32)
    // A Linux cpulist as a mask of logical processors 0-63.
    CHECK(parse_cpu_list("0-7") == 0xFF);
    CHECK(parse_cpu_list("0-11,16-19") == 0xF0FFF);
    CHECK(parse_cpu_list("8-19") == 0xFFF00);
    CHECK(parse_cpu_list("0") == 1);
    CHECK(parse_cpu_list("0,2,4") == 0x15);
    CHECK(parse_cpu_list("0-63") == ~0ull);
    CHECK(parse_cpu_list("0-64") == ~0ull);   // (the mask holds 64 logical processors)
    CHECK(parse_cpu_list("0-7\n") == 0xFF);   // (the file's line ends with a newline)
    CHECK(parse_cpu_list("") == 0);
    CHECK(parse_cpu_list("x") == 0);
    CHECK(parse_cpu_list("5-3") == 0);        // (a range backwards)
    CHECK(parse_cpu_list("1-2-3") == 0);
    CHECK(parse_cpu_list("1,,2") == 0);
    CHECK(parse_cpu_list("0-3 ") == 0);       // (trailing junk is not a list)
#endif

    puts("PASS: the last core of the fastest kind with 4+ of them, the last core on CPUs of one kind, none under 8 "
         "logical processors, and the Linux hybrid cpulist");
    return 0;
}
