// readback_wait (gpu/vk/readback_wait.h): a readback's wait for its write-back ends after 250 ms unless
// RX_VK_READBACK_WAIT_MS says otherwise (0: no limit, as before). No game, no GPU, no wait.
#include "../gpu/vk/readback_wait.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

int main() {
    // (the default: 250 ms)
    CHECK(readback_wait::limit_ms(nullptr) == 250);
    CHECK(readback_wait::timeout_ns(250) == 250000000ull);

    // (a limit set by hand, in whole and fractional milliseconds)
    CHECK(readback_wait::limit_ms("100") == 100);
    CHECK(readback_wait::timeout_ns(100) == 100000000ull);
    CHECK(readback_wait::limit_ms("1") == 1);
    CHECK(readback_wait::timeout_ns(1) == 1000000ull);
    CHECK(readback_wait::limit_ms(" 42") == 42);   // (atoll skips the space, as a player's file may have one)

    // (0 and below: the unlimited wait, as before)
    CHECK(readback_wait::limit_ms("0") == 0);
    CHECK(readback_wait::timeout_ns(0) == UINT64_MAX);
    CHECK(readback_wait::limit_ms("-5") == -5);
    CHECK(readback_wait::timeout_ns(-5) == UINT64_MAX);

    // (a long limit does not wrap: a year is still a finite timeout)
    CHECK(readback_wait::timeout_ns(365ll * 24 * 60 * 60 * 1000) < UINT64_MAX);

    std::printf("PASS: a readback's wait for its write-back is bounded, 250 ms by default\n");
    return 0;
}
