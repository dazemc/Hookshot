// (hookshot) The load default's safety net: when loads at create calls must give way to loading before the title.
// tests/load_default_test.cpp
//
// RX_VK_LOAD=create loads each program's kept variants and noted pipelines at its create call, from the caches only.
// A listed pipeline that is in no cache is compiled when it is first drawn, in play, with the draw left out: that is
// this mode's doing, and what start mode builds before the title. The safety net counts only those. Measured: 0 in
// every tour where the caches held the preparation's work (at most 39 on a GPU without mesh shaders); 4,189 and 5,384
// in the two sessions of a folder whose caches did not answer.
//
// Not counted: draws left out and waits for pipelines no list names. A state first seen in play (a new game's opening:
// 1,906 left out and 353 ms of waits in eight minutes of a healthy session) is left out in start mode just the same, so
// it says nothing about this mode, and a longer session would have taken a healthy install back to start by them.
//
// The load-at-create mode (#2760) and its session counters are not in this tree yet; the rule is kept and tested here
// for when they are ported. program_cache_load_some's loads at the start are start mode, which builds the listed
// pipelines before play by design.
#pragma once

namespace load_default {

// (between the at most 39 a GPU without mesh shaders lacked and the 4,189 of the folder whose caches did not answer)
constexpr long long kNotInCacheMost = 500;

/// Whether the listed pipelines this session's loads found in no cache are enough to load before the title again.
inline bool caches_lacked(long long listed_not_in_a_cache) { return listed_not_in_a_cache > kNotInCacheMost; }

} // namespace load_default
