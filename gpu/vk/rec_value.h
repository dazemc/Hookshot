// Which recording finishes a submission's value (vk_replay.cpp's value_rec; tests/test_rec_value.cpp).
//
// A submission longer than RX_VK_CHUNK draws, or one stopped at an event wait and finished later, is recorded in several
// recordings that all carry its value. The value's recording must be its LAST part's, the latest seq: emplace kept the
// first part's, and a wait for the submission's fence (the game's wait, the readback wait) came back once its first ~120
// draws were done. The game then wrote the next frame's uniforms, vertices and its exposure meter over memory the later
// parts still read (with no VRAM mirror, an iGPU: guest memory read in place): a frame of the HUD stretched across the
// screen, grass and objects glowing white, whole-screen flashes (issue 997).
#pragma once
#include <cstdint>

namespace rec_value {

// (value_rec is a std::map<uint64_t, uint64_t>, value -> the seq of the recording that finished it, or a std::multimap in
// the fixtures that model it with several recordings a value: find and an update in place leave both as they are)
template <class Map>
inline void finished_in(Map& value_rec, uint64_t value, uint64_t seq) {
    auto it = value_rec.find(value);
    if (it == value_rec.end()) value_rec.emplace(value, seq);
    else if (it->second < seq) it->second = seq;
}

// (the same for a recording's part (Recording::part -> its seq on the recordings' timeline): parts are submitted in
// order, but a test and a replay's segments may note one twice; the newest seq stays)
template <class Map>
inline void part_finished_in(Map& part_rec, uint64_t part, uint64_t seq) {
    auto it = part_rec.find(part);
    if (it == part_rec.end()) part_rec.emplace_hint(part_rec.end(), part, seq);
    else if (it->second < seq) it->second = seq;
}

/// The recording that submitted this part: true when known (the seq in *seq), false when it is not (yet, or older
/// than the map keeps).
template <class Map>
inline bool part_known(const Map& part_rec, uint64_t part, uint64_t* seq) {
    auto it = part_rec.find(part);
    if (it == part_rec.end()) return false;
    *seq = it->second;
    return true;
}

/// A part older than the newest one in the map will never be submitted now: a wait for it would wait for nothing.
template <class Map>
inline bool part_impossible(const Map& part_rec, uint64_t part) {
    return !part_rec.empty() && part_rec.rbegin()->first > part;
}

} // namespace rec_value
