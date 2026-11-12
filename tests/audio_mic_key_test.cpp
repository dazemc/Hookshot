/* The microphone name as the Insert menu saves and reads it: control characters become spaces, the ini's line end is
   not part of the name, and trailing spaces (including those an older build saved) are dropped. The functions are cut
   out of gpu/vk/vk_menu.cpp; no game or audio device is opened. */
#include <cstddef>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL:%d %s\n", __LINE__, #x); std::exit(1); } } while (0)

constexpr size_t kMicNameKept = 60;

#include "audio_mic_key.inc"

int main() {
    CHECK(mic_key(nullptr) == "");
    CHECK(mic_key("USB Mic") == "USB Mic");
    /* (the options file's line, read back with its end) */
    CHECK(mic_key("USB Mic\n") == "USB Mic");
    CHECK(mic_key("USB Mic\r\n") == "USB Mic");
    /* (an older build saved the control character as a trailing space, then the load stripped only \r\n) */
    CHECK(mic_key("USB Mic   \n") == "USB Mic");
    CHECK(mic_key("USB Mic   ") == "USB Mic");
    /* (leading spaces are part of the name; only the end is trimmed) */
    CHECK(mic_key("  USB Mic") == "  USB Mic");
    /* (a line break cannot add a line to hookshot_options.ini) */
    CHECK(mic_key("a\tb\nc\rd") == "a b c d");
    CHECK(mic_key("a\x7f b") == "a  b");
    /* (cut to the file's 60 bytes at a character's start, then trimmed) */
    const std::string long_name = std::string(59, 'x') + "\xc3\xa9" + "yy";
    CHECK(mic_key(long_name.c_str()) == std::string(59, 'x'));
    const std::string long_space = std::string(60, 'y') + "  tail";
    CHECK(mic_key(long_space.c_str()) == std::string(60, 'y'));
    /* (a name that is all spaces is empty, as Off's is) */
    CHECK(mic_key("    ") == "");
    std::puts("PASS: a microphone name reads back without its line end or trailing spaces, cut at a character start");
    return 0;
}
