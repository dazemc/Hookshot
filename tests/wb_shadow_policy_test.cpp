// The write-back shadows' lifetime rules (gpu/vk/wb_shadow_policy.h): what is worth a shadow, and what is made again
// after VRAM pressure took it. No game, no GPU.
#include "../gpu/vk/wb_shadow_policy.h"

#include <cstdio>
#include <cstdlib>
#include <vector>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using namespace wb_shadow;

struct Pool {
    uint64_t bytes;
    bool shadow = false;
    bool wanted = false;
    bool mirror = true;
    bool retired = false;
    bool fits = true;
};

/// One shadows_check_room pass, in the production order: candidates with a mirror and no shadow, a skipped one that
/// does not fit does not spend an attempt, at most 8 allocations are tried.
static int check(std::vector<Pool>& pools) {
    int made = 0, tries = 0;
    for (Pool& p : pools) {
        if (p.retired || !remake_candidate(p.wanted, p.shadow, p.mirror)) continue;
        if (!p.fits) continue;
        if (!may_try(tries)) break;
        tries++;
        p.shadow = true;
        made++;
    }
    return made;
}

int main() {
    {   // (a shadow is first made for a write-back of at least 256 KB, when the diff is on and there is none)
        CHECK(!worth_shadow(false, false, 1u << 20));
        CHECK(!worth_shadow(true, true, 1u << 20));
        CHECK(!worth_shadow(true, false, kMakeBytes - 1));
        CHECK(worth_shadow(true, false, kMakeBytes));
        CHECK(worth_shadow(true, false, 1u << 20));
    }
    {   // (a shadow taken by VRAM pressure is made again only when it was worth one, and only while its mirror is there)
        CHECK(!remake_candidate(false, false, true));
        CHECK(!remake_candidate(true, true, true));
        CHECK(!remake_candidate(true, false, false));
        CHECK(remake_candidate(true, false, true));
    }
    {   // (every 120 write-back submissions, and only when there is a shadow to give back or one wanted again)
        CHECK(!check_period(1) && !check_period(119));
        CHECK(check_period(120) && check_period(240));
        CHECK(!check_worth(false, false));
        CHECK(check_worth(true, false) && check_worth(false, true) && check_worth(true, true));
        CHECK(kCheckEvery == 120 && kMakePerCheck == 8);
    }
    {   // (8 allocations at most a check; a candidate too big to fit now spends no try)
        CHECK(may_try(0) && may_try(7));
        CHECK(!may_try(8) && !may_try(9));
        std::vector<Pool> pools;
        for (int i = 0; i < 12; i++) pools.push_back({1u << 20, false, true, true, false, true});
        pools[0].fits = false;
        pools[3].fits = false;
        CHECK(check(pools) == 8);
        int shadows = 0;
        for (const Pool& p : pools) shadows += p.shadow;
        CHECK(shadows == 8);
        CHECK(pools[0].shadow == false && pools[3].shadow == false);   // (skipped without spending an attempt)
        CHECK(pools[1].shadow && pools[2].shadow && pools[4].shadow && pools[5].shadow && pools[6].shadow && pools[7].shadow);
        CHECK(pools[8].shadow && pools[9].shadow);
        CHECK(!pools[10].shadow && !pools[11].shadow);   // (the cap)
        CHECK(check(pools) == 2);                         // (the next check makes the rest)
        CHECK(pools[10].shadow && pools[11].shadow);
    }
    {   // (issue 2238's sequence: 48 pools with shadows given back under pressure, their own write-backs now under
        // 256 KB; the mark stays, so checks make them again, 8 a check, rather than waiting for a write-back that never comes)
        std::vector<Pool> pools;
        for (int i = 0; i < 48; i++) pools.push_back({2u << 20, false, true, true, false, true});
        int made = 0;
        for (int pass = 0; pass < 6; pass++) made += check(pools);
        CHECK(made == 48);
        for (const Pool& p : pools) CHECK(p.shadow);
        // (a pool without the mark, its write-backs under the make size, is never remade)
        pools.push_back({2u << 20, false, false, true, false, true});
        CHECK(check(pools) == 0);
        // (a mirror_free'd pool is not a candidate, and a retired one is never in the loop)
        pools.push_back({2u << 20, false, true, false, false, true});
        pools.push_back({2u << 20, false, true, true, true, true});
        CHECK(check(pools) == 0);
        // (a pool whose shadow is still there is not made twice)
        pools.push_back({2u << 20, true, true, true, false, true});
        CHECK(check(pools) == 0);
    }
    std::printf("PASS: write-back shadow lifetime\n");
    return 0;
}
