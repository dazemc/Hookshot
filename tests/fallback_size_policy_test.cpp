/* fallback_size_policy.h: OpPhi counting, the switch's parsing and the RADV-GCN limit. No game or GPU. */
#include "../gpu/vk/fallback_size_policy.h"

#include <cstdio>

using namespace fallback_size;

int main() {
    const uint32_t module[] = {
        0x07230203u, 0, 0, 1, 0,
        (5u << 16) | 245u, 1, 2, 3, 4,       // OpPhi
        (2u << 16) | 0u, 0,                  // OpNop
        (5u << 16) | 245u, 1, 2, 3, 4,       // OpPhi
    };
    if (spirv_phis(module, sizeof module / sizeof module[0]) != 2) return std::printf("FAIL: phis\n"), 1;
    if (spirv_phis(module, 4) != 0) return std::printf("FAIL: short\n"), 1;
    const uint32_t bad[] = {0, 0, 0, 0, 0, 0};
    if (spirv_phis(bad, 6) != 0) return std::printf("FAIL: magic\n"), 1;

    uint32_t v = 0;
    if (!parse_limit("50000", v) || v != 50000) return std::printf("FAIL: parse\n"), 1;
    if (!parse_limit("0", v) || v != 0) return std::printf("FAIL: zero\n"), 1;
    if (parse_limit("off", v) || parse_limit("-1", v) || parse_limit("1 ", v) || parse_limit("", v))
        return std::printf("FAIL: non-digits\n"), 1;
    if (!too_big(true, 80000, 50000) || too_big(true, 50000, 50000) || too_big(false, 80000, 50000) ||
        too_big(true, 80000, 0))
        return std::printf("FAIL: too_big\n"), 1;
    std::printf("fallback_size_policy: ok\n");
    return 0;
}
