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

} // namespace rec_value
