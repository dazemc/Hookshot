// The write-back page table (gpu/vk/wb_pages.h, vk_memory.cpp's memory_writeback_value): page ranges, the part that
// wrote a page back from a mirror, and pages written in place. No game, no GPU.
#include "../gpu/vk/wb_pages.h"

#include <cstdio>
#include <cstdlib>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

using Table = wb_page_table::Table<12, 4096, 64>;
using Value = wb_page_table::Value;

static Value range(const Table& pages, const Table& parts, uint64_t first, uint64_t last) {
    return wb_page_table::range(pages, parts, first, last);
}

int main() {
    {   // (a page holds its newest value; an address past the table is not a page)
        Table pages, parts;
        CHECK(pages.empty());
        pages.set(3, 11);
        CHECK(!pages.empty() && pages.get(3) == 11 && pages.get(2) == 0);
        pages.set(3, 12);
        CHECK(pages.get(3) == 12);
        pages.set(Table::kPages, 99);
        CHECK(pages.get(Table::kPages) == 0 && pages.get(Table::kPages + 1) == 0);
        CHECK(pages.max_in(3, 3, nullptr) == 12);
        CHECK(pages.max_in(4, 4, nullptr) == 0);
    }
    {   // (a range sees every page across chunks, and skips a chunk that was never made whole)
        Table pages, parts;
        pages.set(1, 5);
        pages.set(64, 7);    // (chunk 1's first page)
        pages.set(200, 9);   // (chunk 3: chunks 0, 1 and 3 exist, 2 does not)
        CHECK(pages.max_in(0, 200, nullptr) == 9);
        CHECK(pages.max_in(65, 199, nullptr) == 0);   // (only page 64 and 200 exist, outside this range)
        CHECK(pages.max_in(128, 191, nullptr) == 0);  // (chunk 2 was never made)
        bool ever = false;
        CHECK(pages.max_in(128, 191, &ever) == 0 && !ever);
        ever = false;
        CHECK(pages.max_in(0, 200, &ever) == 9 && ever);
        ever = false;
        CHECK(pages.max_in(300, 200, &ever) == 0 && !ever);   // (an empty range has nothing)
    }
    {   // (the newest value, and the part that wrote it back from a mirror against a page written in place)
        Table pages, parts;
        pages.set(10, 100);
        parts.set(10, 7);
        pages.set(11, 101);
        parts.set(11, 0);   // (written in place: its whole submission is what a read waits for)
        Value r = range(pages, parts, 10, 11);
        CHECK(r.value == 101 && r.part == 7 && r.whole == 101 && r.ever);
        CHECK(range(pages, parts, 11, 11).value == 101 && range(pages, parts, 11, 11).part == 0);
        CHECK(range(pages, parts, 10, 10).part == 7 && range(pages, parts, 10, 10).whole == 0);
    }
    {   // (a later write of the same page replaces its part: the part and the value are of the same write)
        Table pages, parts;
        pages.set(20, 50);
        parts.set(20, 3);
        pages.set(20, 60);
        parts.set(20, 4);
        Value r = range(pages, parts, 20, 20);
        CHECK(r.value == 60 && r.part == 4 && r.whole == 0);
        // (a later in-place write clears the older mirror part; the read waits for that whole submission)
        pages.set(20, 61);
        parts.set(20, 0);
        r = range(pages, parts, 20, 20);
        CHECK(r.value == 61 && r.part == 0 && r.whole == 61);
        // (and a later mirror write clears the in-place value)
        pages.set(20, 62);
        parts.set(20, 5);
        r = range(pages, parts, 20, 20);
        CHECK(r.value == 62 && r.part == 5 && r.whole == 0);
    }
    {   // (a never-written page is not part of a range's value, even if its chunk exists)
        Table pages, parts;
        pages.set(30, 70);
        parts.set(30, 2);
        Value r = range(pages, parts, 29, 31);
        CHECK(r.value == 70 && r.part == 2 && r.whole == 0 && r.ever);
        r = range(pages, parts, 29, 29);
        CHECK(r.value == 0 && r.part == 0 && r.whole == 0 && !r.ever);
    }
    {   // (the page table keeps a value the GPU is past: readers compare it with their progress, never lower it)
        Table pages, parts;
        pages.set(40, 500);
        parts.set(40, 9);
        pages.set(40, 400);   // (an older note does not take the newest back in production, but the table itself keeps
        parts.set(40, 8);     // what it is given; the caller notes in timeline order)
        CHECK(range(pages, parts, 40, 40).value == 400 && range(pages, parts, 40, 40).part == 8);
    }
    std::printf("PASS: write-back page ranges and part matching\n");
    return 0;
}
