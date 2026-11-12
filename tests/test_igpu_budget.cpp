// The integrated GPU's texture budget (gpu/vk/igpu_budget.h, upstream a435efd0 #2603): the tiers by system RAM, the
// RAM reserved for the rest of the PC, the driver's allowance, and RX_VK_VRAM_MB winning over all of it. No game, no
// GPU: the header is pure arithmetic, and builder/rust/tests/igpu_budget.rs compiles and runs this.
#include "../gpu/vk/igpu_budget.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using namespace mkwvk::igpu_budget;

int main() {
    const uint64_t MB = 1ull << 20, GB = 1ull << 30;
    const uint64_t driver = 6 * GB;   // (an iGPU the driver allows the process 6 GB, already 70% of its own figure)

    // The hard caps by tier, including either side of each boundary.
    CHECK(cap(8 * GB) == 1536 * MB);
    CHECK(cap(9 * GB) == 2048 * MB);
    CHECK(cap(12 * GB) == 2048 * MB);
    CHECK(cap(13 * GB) == 2560 * MB);
    CHECK(cap(16 * GB) == 2560 * MB);
    CHECK(cap(17 * GB) == 3584 * MB);
    CHECK(cap(24 * GB) == 3584 * MB);
    CHECK(cap(25 * GB) == 4096 * MB);
    CHECK(cap(32 * GB) == 4096 * MB);
    CHECK(cap(33 * GB) == kCap);
    CHECK(cap(48 * GB) == kCap);
    CHECK(cap(64 * GB) == kCap);

    // The tiers the fix was written for: 8, 12, 16, 24, 32 and 48 GB.
    {
        const Plan p = choose(8 * GB, driver, false, true);
        CHECK(p.budget == 1024 * MB);   // (the 8 GB reserve leaves nothing, so the 1 GB floor, under the 1.5 GB cap)
        CHECK(p.reserved == 8 * GB);
    }
    {
        const Plan p = choose(12 * GB, driver, false, true);
        CHECK(p.budget == 2048 * MB);
        CHECK(p.reserved == 8 * GB);
    }
    {
        const Plan p = choose(16 * GB, driver, false, true);
        CHECK(p.budget == 2560 * MB);   // (the old fixed budget)
        CHECK(p.reserved == 8 * GB);
    }
    {
        const Plan p = choose(24 * GB, driver, false, true);
        CHECK(p.budget == 3584 * MB);
        CHECK(p.reserved == 8 * GB);
    }
    {
        const Plan p = choose(32 * GB, driver, false, true);
        CHECK(p.budget == 4096 * MB);
        CHECK(p.reserved == (32 * GB) / 3);   // (10 GB: over the 8 GB floor, under the 16 GB ceiling)
    }
    {
        const Plan p = choose(48 * GB, driver, false, true);
        CHECK(p.budget == 6144 * MB);
        CHECK(p.reserved == 16 * GB);   // (system_ram / 3 is exactly 16 GB here)
    }
    {
        const Plan p = choose(64 * GB, driver, false, true);
        CHECK(p.budget == 6144 * MB);   // (the discrete cards' own cap)
        CHECK(p.reserved == 16 * GB);   // (the reserve is capped at 16 GB)
    }

    // The reserve's 8 GB floor and what it leaves: 9 GB has 1 GB available, 10 GB has 2 GB (the tier's cap).
    CHECK(choose(9 * GB, driver, false, true).budget == 1024 * MB);
    CHECK(choose(10 * GB, driver, false, true).budget == 2048 * MB);

    // A machine whose RAM could not be read is the 8 GB tier, not no budget.
    {
        const Plan p = choose(0, driver, false, true);
        CHECK(p.budget == 1024 * MB);
        CHECK(p.reserved == 8 * GB);
    }

    // The driver's own allowance wins over the tier: a 32 GB machine whose driver allows 1.5 GB keeps 1.5 GB.
    CHECK(choose(32 * GB, 1500 * MB, false, true).budget == 1500 * MB);
    // And under the 1 GB floor too: never more than the driver allows.
    CHECK(choose(32 * GB, 512 * MB, false, true).budget == 512 * MB);
    CHECK(choose(8 * GB, 700 * MB, false, true).budget == 700 * MB);
    CHECK(choose(8 * GB, 1500 * MB, false, true).budget == 1024 * MB);   // (the 1 GB floor, under the 1.5 GB tier)
    CHECK(choose(32 * GB, 0, false, true).budget == 0);

    // RX_VK_VRAM_MB wins over everything, even over the 6 GB cap and on a machine whose RAM could not be read.
    {
        const Plan p = choose(8 * GB, 10 * GB, true, true);
        CHECK(p.budget == 10 * GB);
        CHECK(p.reserved == 0);   // (no tier was used)
    }
    CHECK(choose(0, 5000 * MB, true, true).budget == 5000 * MB);

    // A discrete GPU is untouched by the tiers: its own budget, whatever the system RAM.
    CHECK(choose(8 * GB, 5000 * MB, false, false).budget == 5000 * MB);
    CHECK(choose(48 * GB, 5000 * MB, false, false).budget == 5000 * MB);
    CHECK(choose(16 * GB, 2560 * MB, false, false).budget == 2560 * MB);
    CHECK(choose(8 * GB, 10 * GB, false, false).budget == 10 * GB);   // (the caller's own 6 GB cap already applied)

    std::printf("PASS: the integrated GPU's budget follows the system RAM tiers, the reserve and the driver, and "
                "RX_VK_VRAM_MB and discrete GPUs keep their own\n");
    return 0;
}
