// (hookshot) The temporal chain's FSR 3.1 engine and the game's native HDR: the colour-space handoff's host decision, compiled
// from the renderer's own source by builder/rust/tests/fsr3_hdr.rs. No game, dump or GPU.
//
// chain_hdr() is vk_fsr3.cpp's: RX_VK_FSR3_HDR=0 keeps the LDR pair for comparisons, else the engine takes the game's HDR
// colour as linear light exactly while the game presents its own PQ picture (present_native_pq). The fixture runs once per
// switch value: the function caches the switch at its first ask, as it does in the renderer.
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <string>

std::atomic<bool> present_native_pq{false};

/* ENV_SWITCH */

/* CHAIN_HDR */

static int failures = 0;
static void check(const char* name, bool value) {
    if (!value) {
        std::fprintf(stderr, "FAIL: %s\n", name);
        failures++;
    }
}

int main(int argc, char** argv) {
    const bool native = argc < 2 || std::string(argv[1]) != "native-off";
    present_native_pq.store(native, std::memory_order_relaxed);
    const char* e = std::getenv("RX_VK_FSR3_HDR");
    const bool allowed = !(e && *e && std::atoi(e) == 0);
    check("the game's native PQ picture selects the chain's HDR colour", chain_hdr() == (native && allowed));
    // (a second ask: the cached switch must not change its answer)
    check("the decision is stable", chain_hdr() == (native && allowed));
    if (failures) return 1;
    std::printf("fsr3 hdr: %s colour%s\n", native && allowed ? "HDR" : "LDR",
                native && !allowed ? " (RX_VK_FSR3_HDR=0)" : "");
    return 0;
}
