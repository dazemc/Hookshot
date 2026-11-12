/* load_default.h: the load default's safety net counts only the listed pipelines the loads found in no cache, against
   the measured sessions. No game, no GPU. */
#include "../gpu/vk/load_default.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

int main() {
    using load_default::caches_lacked;
    // the clean tours had none (at most 39 without mesh shaders); at the limit it has not fired
    CHECK(!caches_lacked(0) && !caches_lacked(39) && !caches_lacked(500));
    // the two sessions of the folder whose caches did not answer had 4,189 and 5,384
    CHECK(caches_lacked(501) && caches_lacked(3720 + 469) && caches_lacked(4718 + 666));
    // a healthy session's draws left out for pipelines no list names (1,906, and 353 ms of waits, in eight minutes of a
    // new game's opening; the same in start mode) are no input of the rule at all: with 0 listed pipelines missing it
    // does not trip, however long the session
    CHECK(!caches_lacked(0));
    std::printf("PASS: only listed pipelines no cache had count toward the load default's safety net\n");
    return 0;
}
