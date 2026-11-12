// (hookshot) A warp's shared memory store (STS) and its next load (LDS) get a subgroup barrier between them
// (gpu/vk/eden/shader_recompiler/frontend/ampere/lower_to_maxwell.h). The Switch runs a warp's shared memory accesses in
// order, so OoT's programs store to shared memory and read it back, other lanes' words too, with no BAR between. Vulkan
// orders another lane's store and this lane's load only through a barrier, and the driver's compiler may move them past
// each other: OoT's 4/3 gameplay network (fd029a317a9a4b94) ends with 8 STS then 16 LDS, and on NVIDIA's cooperative
// matrix path the picture got flickering bright bars 16 px wide and 2 rows tall (issue 2455). As for LDSM: a subgroup
// barrier (MEMBAR, bit 20) before the first LDS after an STS, only where the LDS is not predicated (every lane of the
// warp reaches it); none after a BAR.SYNC. RX_VK_SHARED_NO_LDS_BARRIER=1: none, as before.
//
//   lower_lds_after_sts_test            the checks
//   lower_lds_after_sts_test --switch-off  the switch's case, in a process of its own (the lowering reads it once)
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "shader_recompiler/frontend/ampere/lower_to_maxwell.h"

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using Shader::Ampere::Instruction;

static Instruction insn(uint32_t a, uint32_t b, uint32_t c, uint32_t d) {
    Instruction i{};
    i.words[0] = a, i.words[1] = b, i.words[2] = c, i.words[3] = d;
    return i;
}
static const Instruction exit_insn = insn(0x0000794d, 0, 0x03800000, 0x000fea00);   // EXIT
// STS.32 [R2+0x10], R5 and LDS.32 R6, [R3+0x20] (@PT; @P0 for the predicated one); BAR.SYNC 0
static const Instruction sts = insn(0x02ff7388, 0x00001005, 0x00000800, 0x000fe800);
static const Instruction lds = insn(0x03067984, 0x00002000, 0x00000800, 0x000e2800);
static const Instruction lds_p0 = insn(0x03060984, 0x00002000, 0x00000800, 0x000e2800);
static const Instruction bar = insn(0x00007b1d, 0, 0x00010000, 0x000fec00);

static bool subgroup_barrier(uint64_t i) { return (i & 0xfff8000000000000ULL) == 0xef98000000000000ULL && ((i >> 20) & 1); }
static bool maxwell_lds(uint64_t i) { return (i >> 51) == (0xef48000000000000ULL >> 51); }   // (the size is in 48..50)
static bool maxwell_sts(uint64_t i) { return (i >> 51) == (0xef58000000000000ULL >> 51); }

// The lowered program as a string: B a subgroup barrier, L an LDS, S an STS, . anything else
static std::string shape(std::vector<Instruction> code) {
    code.push_back(exit_insn);
    const auto r = Shader::Ampere::LowerToMaxwell(code);
    CHECK(r);
    std::string s;
    for (const uint64_t i : r.instructions)
        s += subgroup_barrier(i) ? 'B' : maxwell_lds(i) ? 'L' : maxwell_sts(i) ? 'S' : '.';
    return s;
}
static size_t count(const std::string& s, char c) {
    size_t n = 0;
    for (char x : s) n += x == c;
    return n;
}

int main(int argc, char** argv) {
    if (argc == 2 && !std::strcmp(argv[1], "--switch-off")) {   // RX_VK_SHARED_NO_LDS_BARRIER=1, set by the run below
        CHECK(count(shape({sts, lds}), 'B') == 0);
        std::printf("PASS: RX_VK_SHARED_NO_LDS_BARRIER=1 leaves the LDS after an STS without a barrier\n");
        return 0;
    }
    CHECK(argc == 1);
#ifdef _WIN32
    CHECK(_putenv_s("RX_VK_SHARED_NO_LDS_BARRIER", "") == 0);
#else
    unsetenv("RX_VK_SHARED_NO_LDS_BARRIER");
#endif
    const std::string one = shape({sts, lds});
    CHECK(count(one, 'B') == 1);
    CHECK(one.find('S') < one.find('B') && one.find('B') < one.find('L'));   // between the store and the load
    CHECK(count(shape({sts, lds, lds}), 'B') == 1);          // the first load after the store only
    CHECK(count(shape({sts, lds, sts, lds}), 'B') == 2);     // a store again: the next load again
    CHECK(count(shape({lds}), 'B') == 0);                    // no store before it
    CHECK(count(shape({sts, bar, lds}), 'B') == 0);          // a BAR.SYNC orders them already
    CHECK(count(shape({sts, lds_p0}), 'B') == 0);            // predicated: not every lane may reach it
    CHECK(count(shape({sts, lds_p0, lds}), 'B') == 1);       // the next unpredicated load takes it
    std::printf("PASS: a subgroup barrier between a warp's STS and its next LDS\n");

    // (the switch reads the environment once, in this process the lowering has already read it: the other case runs in a
    // child of its own)
#ifdef _WIN32
    CHECK(_putenv_s("RX_VK_SHARED_NO_LDS_BARRIER", "1") == 0);
#else
    CHECK(setenv("RX_VK_SHARED_NO_LDS_BARRIER", "1", 1) == 0);
#endif
    const std::string cmd = std::string("\"") + argv[0] + "\" --switch-off";
    CHECK(std::system(cmd.c_str()) == 0);
    return 0;
}
