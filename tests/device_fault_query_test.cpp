/* device_fault_query.h: when a lost device's report asks VK_EXT_device_fault automatically, the NVIDIA guard, and the
   off switch. No game, no GPU. */
#include "../gpu/vk/device_fault_query.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using device_fault_query::automatic;
using device_fault_query::forced;
using device_fault_query::wanted;

int main() {
    // RX_VK_FAULT_INFO: on for 1 or any other value, off when unset or off-like
    CHECK(forced("1") && forced("yes") && forced("2"));
    CHECK(!forced(nullptr) && !forced("") && !forced("0") && !forced("false") && !forced("off") && !forced("no"));

    // no checkpoints, no markers, AMD or Intel: asked with no switch at all
    CHECK(automatic(false, false, false, nullptr));
    CHECK(automatic(false, false, false, "1"));
    // NVIDIA: never automatic, even when the checkpoints are switched off (vkGetDeviceFaultInfoEXT can hang there)
    CHECK(!automatic(true, false, false, nullptr));
    // checkpoints or buffer markers: the report has something else to read
    CHECK(!automatic(false, true, false, nullptr));
    CHECK(!automatic(false, false, true, nullptr));
    CHECK(!automatic(false, true, true, nullptr));
    // RX_VK_FAULT_AUTO=0: off on any GPU; the off-like spellings count
    const char* const offs[] = {"0", "false", "off", "no", "", " off "};
    for (const char* off : offs) CHECK(!automatic(false, false, false, off));
    CHECK(automatic(false, false, false, "yes"));

    // the last report: the switch wins; without it, the automatic ask
    CHECK(wanted(false, false, false, nullptr, nullptr));                 // AMD/Intel, automatic
    CHECK(wanted(false, false, false, "1", "0"));                         // the switch over the off switch
    CHECK(!wanted(false, false, false, nullptr, "0"));                    // automatic off
    CHECK(wanted(false, false, false, "0", nullptr));                     // RX_VK_FAULT_INFO=0 does not force, the automatic ask still runs
    CHECK(!wanted(false, false, false, "0", "0"));                        // both switches off: no report
    CHECK(!wanted(true, false, false, nullptr, nullptr));                 // NVIDIA guard
    CHECK(wanted(true, false, false, "1", nullptr));                      // by hand on NVIDIA, as before
    CHECK(!wanted(false, true, false, nullptr, nullptr));                 // checkpoints answer
    CHECK(!wanted(false, false, true, nullptr, nullptr));                 // markers answer

    std::printf("PASS: the fault info is asked automatically off NVIDIA, and can be switched off\n");
    return 0;
}
