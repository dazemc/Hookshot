// (hookshot) The temporal chain's engine choice and its fallbacks, compiled from vk_temporal.cpp's temporal_mode by
// builder/rust/tests/fsr3_hdr.rs: FSR 3.1 on the chain before the chain stands aside, RX_VK_FSR3_CHAIN=0's own-inputs
// path as GAME, the row's engines only where they run, and the DLSS/FSR 4 environment overrides over the row. No game,
// dump or GPU.
#include <cstdio>

struct {
    int dlss = 0;
} opts;

namespace upscaler_rows {
enum { GAME, DLSS, DLAA, FSR3, FSR4, FSR4_AA, OFF };
inline bool is_dlss(int v) { return v == DLSS || v == DLAA; }
inline bool is_fsr4(int v) { return v == FSR4 || v == FSR4_AA; }
}  // namespace upscaler_rows
namespace ur = upscaler_rows;

static int env_dlss_value = -1, env_fsr4_value = -1;
static int dlss_reason = 0, fsr4_reason = 0;
static bool chain_on = true, chosen = true, own_inputs = false;
static constexpr int DLSS_OK = 0, FSR_OK = 0;

int env_dlss() { return env_dlss_value; }
int env_fsr4() { return env_fsr4_value; }
int dlss_why() { return dlss_reason; }
int fsr4_why() { return fsr4_reason; }
bool fsr3_on_chain() { return chain_on; }
bool fsr3_chosen() { return chosen; }
bool fsr3_wanted() { return own_inputs; }

/* TEMPORAL_MODE */

static int failures = 0;
static void check(const char* name, int got, int want) {
    if (got != want) {
        std::fprintf(stderr, "FAIL: %s: %d, wanted %d\n", name, got, want);
        failures++;
    }
}

int main() {
    namespace ur = upscaler_rows;
    opts.dlss = ur::FSR3;
    check("FSR 3.1 on the chain is its engine", temporal_mode(), ur::FSR3);

    chain_on = false, own_inputs = true;
    check("RX_VK_FSR3_CHAIN=0 leaves the chain to the game", temporal_mode(), ur::GAME);

    chain_on = true, own_inputs = false, chosen = false;
    check("FSR 3.1 that cannot run does not draw the chain", temporal_mode(), ur::GAME);

    opts.dlss = ur::DLSS;
    check("DLSS where it runs", temporal_mode(), ur::DLSS);
    dlss_reason = 1;
    check("DLSS that cannot run falls back to the game", temporal_mode(), ur::GAME);
    dlss_reason = 0;

    opts.dlss = ur::FSR4;
    check("FSR 4 where it runs", temporal_mode(), ur::FSR4);
    fsr4_reason = 1;
    check("FSR 4 that cannot run falls back to the game", temporal_mode(), ur::GAME);
    fsr4_reason = 0;

    opts.dlss = ur::OFF;
    check("Upscaler Off is no engine", temporal_mode(), ur::GAME);

    opts.dlss = ur::FSR3;
    // (fsr3_chosen is false while an environment switch runs its engine over the row: vk_fsr3.cpp's temporal_env_chosen)
    chosen = false, env_dlss_value = 1;
    check("RX_VK_DLSS=1 goes over the row's FSR 3.1", temporal_mode(), ur::DLSS);
    env_dlss_value = 2;
    check("RX_VK_DLSS=2 is DLAA", temporal_mode(), ur::DLAA);
    env_dlss_value = -1, env_fsr4_value = 1;
    check("RX_VK_FSR4=1 goes over the row's FSR 3.1", temporal_mode(), ur::FSR4);
    env_fsr4_value = 2;
    check("RX_VK_FSR4=2 is FSR 4's native AA", temporal_mode(), ur::FSR4_AA);

    if (failures) return 1;
    std::printf("fsr3 fallback: all checks passed\n");
    return 0;
}
