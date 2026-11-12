// SPDX-License-Identifier: GPL-3.0-or-later
// (hookshot, #1001, #1971) How the game's task and mesh programs run: 0 Automatic (the GPU's own mesh shaders where it has
// them, the compute fallback where not, and for the task + mesh draws of the GPUs vk_main.cpp names), 1 Native (no fallback
// where there are mesh shaders), 2 Compute (the fallback on every GPU). The Graphics tab's Mesh shaders row is gone, as
// changing it translated every shader again and dropped the pipeline caches for no gain a player could see. A launch
// switch, RX_VK_MESH_SHADERS, chooses now. An old mesh_shaders= line in hookshot_options.ini still chooses as its row did
// (the menu keeps the line), so an existing file keeps the path it had; RX_VK_MESH_SHADERS over it. Nothing of Vulkan
// here: tests/test_mesh_mode_switch.cpp.
#pragma once
#include <cctype>
#include <cstdlib>
#include <cstring>

namespace mesh_rule {

struct Choice {
    int mode;          // 0 Automatic, 1 Native, 2 Compute
    const char* why;   // where it came from, for the log
};

inline bool same_word(const char* a, const char* b) {
    for (; *a && *b; a++, b++)
        if (std::tolower((unsigned char)*a) != *b) return false;
    return !*a && !*b;
}

/// env: RX_VK_MESH_SHADERS's value or null (native or compute; anything else, Automatic). force_fallback:
/// RX_VK_FORCE_MESH_FALLBACK's value or null, a test that is Compute over it when set and not 0, as it was over the row.
/// old_ini: the old row's mesh_shaders= value from hookshot_options.ini (0 Automatic, 1 Native, 2 Compute), read when the
/// switch is unset; the menu no longer writes it, and keeps an existing line so that file keeps choosing.
inline Choice choose(const char* env, const char* force_fallback, int old_ini = 0) {
    if (force_fallback && std::atoi(force_fallback) != 0) return {2, "RX_VK_FORCE_MESH_FALLBACK"};
    if (env && same_word(env, "native")) return {1, "RX_VK_MESH_SHADERS"};
    if (env && same_word(env, "compute")) return {2, "RX_VK_MESH_SHADERS"};
    if (env && *env) return {0, "the default (RX_VK_MESH_SHADERS is neither native nor compute)"};
    if (old_ini == 1) return {1, "mesh_shaders= in hookshot_options.ini (the old row's Native)"};
    if (old_ini == 2) return {2, "mesh_shaders= in hookshot_options.ini (the old row's Compute)"};
    return {0, "the default"};
}

/// A line of hookshot_options.ini: the old row's mesh_shaders= with Native (1) or Compute (2). *value: what it held.
/// Automatic's 0 and any other value are no choice (the row read 0 to 2, anything else Automatic).
inline bool old_ini_choice(const char* line, int* value) {
    if (std::strncmp(line, "mesh_shaders=", 13)) return false;
    *value = std::atoi(line + 13);
    return *value == 1 || *value == 2;
}

}  // namespace mesh_rule
