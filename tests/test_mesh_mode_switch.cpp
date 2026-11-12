// mesh_rule (gpu/vk/mesh_mode.h): how task and mesh programs run is a launch switch, and an old options file's
// mesh_shaders= line still chooses as its row did. No game, GPU or options file is read: tests/test_mesh_mode_switch.rs
// compiles and runs this.
#include "../gpu/vk/mesh_mode.h"

#include <cstdio>
#include <cstring>
#include <initializer_list>

static int failures = 0;
static void expect(bool ok, const char* what) {
    if (!ok) {
        std::printf("FAIL: %s\n", what);
        failures++;
    }
}

int main() {
    using mesh_rule::choose;
    // (the switch chooses; any other value, or none, is Automatic)
    expect(choose(nullptr, nullptr).mode == 0 && !std::strcmp(choose(nullptr, nullptr).why, "the default"),
           "unset: Automatic");
    expect(choose("native", nullptr).mode == 1 && !std::strcmp(choose("native", nullptr).why, "RX_VK_MESH_SHADERS"),
           "native: Native");
    expect(choose("compute", nullptr).mode == 2 && !std::strcmp(choose("compute", nullptr).why, "RX_VK_MESH_SHADERS"),
           "compute: Compute");
    expect(choose("Native", nullptr).mode == 1 && choose("COMPUTE", nullptr).mode == 2, "the words are case-insensitive");
    expect(choose("", nullptr).mode == 0, "an empty switch: Automatic");
    for (const char* other : {"1", "2", "0", "automatic", "nativ", "compute2", " native", "fallback"}) {
        expect(choose(other, nullptr).mode == 0, "another value: Automatic");
        expect(std::strstr(choose(other, nullptr).why, "RX_VK_MESH_SHADERS") != nullptr, "another value: the log says why");
    }
    // (RX_VK_FORCE_MESH_FALLBACK, set and not 0, is Compute over it; 0 leaves the switch to choose)
    expect(choose("native", "1").mode == 2 && !std::strcmp(choose("native", "1").why, "RX_VK_FORCE_MESH_FALLBACK"),
           "the test switch wins: Compute");
    expect(choose(nullptr, "1").mode == 2, "the test switch alone: Compute");
    expect(choose("native", "0").mode == 1 && choose(nullptr, "0").mode == 0, "0 leaves the choice");

    // (an old mesh_shaders= line still chooses as the row did when the launch switch is unset: the menu keeps it)
    expect(choose(nullptr, nullptr, 1).mode == 1 && std::strstr(choose(nullptr, nullptr, 1).why, "mesh_shaders="),
           "an old Native line: Native");
    expect(choose(nullptr, nullptr, 2).mode == 2 && std::strstr(choose(nullptr, nullptr, 2).why, "mesh_shaders="),
           "an old Compute line: Compute");
    expect(choose(nullptr, nullptr, 0).mode == 0, "an old Automatic line: Automatic");
    expect(choose(nullptr, nullptr, 7).mode == 0, "an old value past the row: Automatic");
    // (the launch switch wins over the old line, and the test switch over both)
    expect(choose("native", nullptr, 2).mode == 1, "the switch wins over an old Compute line");
    expect(choose("compute", nullptr, 1).mode == 2, "the switch wins over an old Native line");
    expect(choose("other", nullptr, 2).mode == 0, "a switch value that chooses nothing is Automatic, not the old line");
    expect(choose(nullptr, "1", 1).mode == 2, "RX_VK_FORCE_MESH_FALLBACK wins over the old line");

    // (the line parser: only the old row's Native and Compute are choices)
    int v = -1;
    expect(mesh_rule::old_ini_choice("mesh_shaders=1\n", &v) && v == 1, "mesh_shaders=1 is read");
    expect(mesh_rule::old_ini_choice("mesh_shaders=2\r\n", &v) && v == 2, "mesh_shaders=2 is read");
    expect(!mesh_rule::old_ini_choice("mesh_shaders=0\n", &v), "mesh_shaders=0 is no choice");
    expect(!mesh_rule::old_ini_choice("mesh_shaders=7\n", &v), "mesh_shaders=7 is no choice");
    expect(!mesh_rule::old_ini_choice("vram_budget=2\n", &v), "another key is not the row");
    expect(!mesh_rule::old_ini_choice("# mesh_shaders=1\n", &v), "a comment is not the row");

    if (failures) return 1;
    std::puts("mesh mode: RX_VK_MESH_SHADERS chooses, RX_VK_FORCE_MESH_FALLBACK over it, an old line still chooses");
    return 0;
}
