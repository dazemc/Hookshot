// gpu/vk/translate_wait.h: RX_VK_TRANSLATE_WAIT_MS parsed strictly, so a typo ("8ms", "off") does not read as 0 and leave
// every untranslated draw out at once. No game, dump or GPU.
#include "../gpu/vk/translate_wait.h"

#include <cmath>
#include <cstdio>

static int failed;
#define CHECK(x)                                                \
    do {                                                        \
        if (!(x)) {                                             \
            std::printf("FAILED line %d: %s\n", __LINE__, #x);  \
            failed++;                                           \
        }                                                       \
    } while (0)

int main() {
    double v = 0;
    CHECK(!translate_wait::parse(nullptr, v));
    CHECK(!translate_wait::parse("", v));
    CHECK(!translate_wait::parse("  ", v));
    CHECK(!translate_wait::parse("8ms", v));
    CHECK(!translate_wait::parse("off", v));
    CHECK(!translate_wait::parse("nan", v));
    CHECK(!translate_wait::parse("inf", v));
    CHECK(!translate_wait::parse("8 9", v));

    CHECK(translate_wait::parse("8", v) && v == 8);
    CHECK(translate_wait::parse("2.5", v) && v == 2.5);
    CHECK(translate_wait::parse(" 8 \t\r\n", v) && v == 8);
    CHECK(translate_wait::parse("-1", v) && v == translate_wait::kOff);
    CHECK(translate_wait::parse("0", v) && v == 0);
    CHECK(translate_wait::parse("16.25", v) && std::fabs(v - 16.25) < 1e-9);

    if (failed) return 1;
    std::printf("translate_wait: ok\n");
    return 0;
}
