// rec_value (gpu/vk/rec_value.h): which recording finishes a submission's value, for a submission recorded in parts.
// (no game, no GPU)
#include "../gpu/vk/rec_value.h"

#include <cstdio>
#include <cstdlib>
#include <map>

static int failures = 0;
static void expect(bool ok, const char* what) {
    if (!ok) {
        std::printf("FAIL: %s\n", what);
        failures++;
    }
}
static uint64_t seq_of(const std::map<uint64_t, uint64_t>& m, uint64_t v) {
    auto it = m.find(v);
    return it == m.end() ? 0 : it->second;
}

int main() {
    // (a submission split in three parts: the value is registered with each part, and a wait must find the last one)
    std::map<uint64_t, uint64_t> value_rec;
    rec_value::finished_in(value_rec, 10, 1);
    rec_value::finished_in(value_rec, 10, 2);
    rec_value::finished_in(value_rec, 10, 3);
    expect(seq_of(value_rec, 10) == 3, "a split submission's value names its last part");
    expect(value_rec.size() == 1, "its parts leave one entry");

    // (a stopped submission's value, noted for the parts before the one that signals, and the recording that finishes it)
    rec_value::finished_in(value_rec, 11, 4);
    rec_value::finished_in(value_rec, 11, 5);
    expect(seq_of(value_rec, 11) == 5, "a stopped submission finished later names the recording that finished it");

    // (out of order, or asked twice with an older part: the newest seq stays)
    rec_value::finished_in(value_rec, 11, 4);
    expect(seq_of(value_rec, 11) == 5, "an older part does not take a value back");
    rec_value::finished_in(value_rec, 12, 9);
    rec_value::finished_in(value_rec, 12, 7);
    expect(seq_of(value_rec, 12) == 9, "the latest recording wins whatever order the parts are noted in");
    expect(value_rec.size() == 3, "each value one entry");

    // (a recording's part -> the seq that submitted it: parts are submitted in order, and a wait for an older part
    // must not mistake a newer one for it)
    std::map<uint64_t, uint64_t> part_rec;
    rec_value::part_finished_in(part_rec, 1, 10);
    rec_value::part_finished_in(part_rec, 2, 11);
    rec_value::part_finished_in(part_rec, 3, 12);
    uint64_t seq = 0;
    expect(rec_value::part_known(part_rec, 2, &seq) && seq == 11, "a part names the recording that submitted it");
    expect(!rec_value::part_known(part_rec, 4, &seq), "a part not submitted yet is not known");
    expect(!rec_value::part_impossible(part_rec, 4), "a newer part may still come");
    expect(rec_value::part_impossible(part_rec, 0), "a part older than the newest in the map will not come");
    rec_value::part_finished_in(part_rec, 2, 13);   // (noted again: the newest seq stays, as a fence wait needs)
    expect(rec_value::part_known(part_rec, 2, &seq) && seq == 13, "a part noted twice keeps the newest recording");
    rec_value::part_finished_in(part_rec, 2, 9);
    expect(rec_value::part_known(part_rec, 2, &seq) && seq == 13, "an older note does not take a part back");
    expect(part_rec.size() == 3, "parts leave one entry each");

    if (failures) {
        std::printf("FAILED: %d\n", failures);
        return 1;
    }
    std::puts("PASS: a value names the recording that finished it");
    return 0;
}
