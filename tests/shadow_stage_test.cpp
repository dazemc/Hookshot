// shadow_stage_policy.h: the texture shadow's staging blocks (the size a capture asks for, the free list that keeps at
// most four blocks and 96 MB, RX_VK_LOW_SPEC and RX_VK_SHADOW_STAGE's values). No game, no GPU.
#include "../gpu/vk/shadow_stage_policy.h"

#include <cstdio>
#include <cstdlib>
#include <utility>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using shadow_stage::Block;
using shadow_stage::FreeList;
using shadow_stage::kMaxBlocks;
using shadow_stage::kMaxBytes;

int main() {
    // (the size: rounded up to 1 MB, so blocks of near sizes fit each other; nothing asked for, nothing made)
    CHECK(shadow_stage::block_bytes(0) == 0);
    CHECK(shadow_stage::block_bytes(1) == 1u << 20);
    CHECK(shadow_stage::block_bytes(4096) == 1u << 20);
    CHECK(shadow_stage::block_bytes(1u << 20) == 1u << 20);
    CHECK(shadow_stage::block_bytes((1u << 20) + 1) == 2u << 20);
    CHECK(shadow_stage::block_bytes(5000 * 4096) == 20u << 20);   // (a submit's texture pages: 19.5 MB)

    // (RX_VK_SHADOW_STAGE: on unless an off value, as every switch; unset is on)
    CHECK(shadow_stage::enabled(nullptr));
    CHECK(shadow_stage::enabled("1"));
    CHECK(shadow_stage::enabled("on"));
    CHECK(shadow_stage::enabled("true"));
    CHECK(shadow_stage::enabled("0x0"));
    CHECK(!shadow_stage::enabled("0"));
    CHECK(!shadow_stage::enabled("off"));
    CHECK(!shadow_stage::enabled("false"));
    CHECK(!shadow_stage::enabled("no"));
    CHECK(!shadow_stage::enabled(" No "));
    CHECK(!shadow_stage::enabled(""));

    {   // (a fresh block is the rounded size; the list keeps it and hands it back)
        FreeList free;
        Block b = free.take(5000 * 4096);
        CHECK(b && b.bytes == 20u << 20);
        CHECK(free.blocks() == 0 && free.bytes() == 0);
        uint8_t* const at = b.get();
        free.put(std::move(b), false);
        CHECK(free.blocks() == 1 && free.bytes() == 20u << 20);
        Block again = free.take(4096);
        CHECK(again.get() == at && again.bytes == 20u << 20);
        CHECK(free.blocks() == 0 && free.bytes() == 0);
    }

    {   // (the smallest block that fits, not the first: the big one stays for a later capture)
        FreeList free;
        free.put(shadow_stage::make(4u << 20), false);
        free.put(shadow_stage::make(2u << 20), false);
        free.put(shadow_stage::make(3u << 20), false);
        CHECK(free.blocks() == 3 && free.bytes() == 9u << 20);
        Block b = free.take(1u << 20);
        CHECK(b && b.bytes == 2u << 20);
        b = free.take(2u << 20);
        CHECK(b && b.bytes == 3u << 20);
        b = free.take(2u << 20);
        CHECK(b && b.bytes == 4u << 20);
        CHECK(free.blocks() == 0 && free.bytes() == 0);
        CHECK(!free.take_kept(1));   // (nothing kept: the caller makes one)
    }

    {   // (four blocks and 96 MB in all: the fifth 24 MB block is freed, not kept)
        FreeList free;
        for (size_t i = 0; i < kMaxBlocks; i++) free.put(shadow_stage::make(24u << 20), false);
        CHECK(free.blocks() == kMaxBlocks && free.bytes() == kMaxBytes);
        free.put(shadow_stage::make(24u << 20), false);
        CHECK(free.blocks() == kMaxBlocks && free.bytes() == kMaxBytes);
        free.put(shadow_stage::make(1u << 20), false);
        CHECK(free.blocks() == kMaxBlocks && free.bytes() == kMaxBytes);
    }

    {   // (three 32 MB blocks are the 96 MB; a fourth is over it even though the count has room)
        FreeList free;
        for (int i = 0; i < 3; i++) free.put(shadow_stage::make(32u << 20), false);
        CHECK(free.blocks() == 3 && free.bytes() == kMaxBytes);
        free.put(shadow_stage::make(1u << 20), false);
        CHECK(free.blocks() == 3 && free.bytes() == kMaxBytes);
    }

    {   // (RX_VK_LOW_SPEC: RAM the tight resource there, so no block is kept; the one taken still works)
        FreeList free;
        Block b = free.take(1u << 20);
        CHECK(b && b.bytes == 1u << 20);
        free.put(std::move(b), true);
        CHECK(free.blocks() == 0 && free.bytes() == 0);
    }

    {   // (an empty block is nothing to keep)
        FreeList free;
        free.put({}, false);
        CHECK(free.blocks() == 0 && free.bytes() == 0);
    }

    std::printf("PASS: the texture shadow's staging blocks are 1 MB rounded, best-fit reused, at most 4 / 96 MB, "
                "none under RX_VK_LOW_SPEC, off with RX_VK_SHADOW_STAGE=0\n");
    return 0;
}
