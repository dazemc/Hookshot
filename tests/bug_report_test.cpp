// The Insert menu's Bug report writes what an issue needs, and nothing private (gpu/vk/vk_bug_report.cpp).
// Game-free: the writer is compiled alone with this driver, which writes reports from made-up game folders in a made-up
// home folder. It checks the folder's name and contents, report.txt's sections, the options and switch values, the logs
// (redacted, NUL bytes kept, a long one cut), the picture, and that no game file or key goes in. The wiring (the menu's
// row, the host's map line) is checked by builder/rust/tests/bug_report.rs, which compiles and runs this file.
#include "../gpu/vk/vk_bug_report.h"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <filesystem>
#include <fstream>
#include <string>
#include <thread>
#include <vector>

namespace fs = std::filesystem;
using namespace mkwvk::bug_report;

static int fails;
#define CHECK(x) do { if (!(x)) { std::fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #x); fails++; } } while (0)
#define CHECK_SAYS(x, says) do { if (!(x)) { std::fprintf(stderr, "FAIL line %d: %s (%s)\n", __LINE__, #x, (says)); fails++; } } while (0)

static const char* kVERSION = "v2.1.0-rc.3-39-gbdfe09c (source 763271ad)";

static std::string read_text(const fs::path& p) {
    std::ifstream f(p, std::ios::binary);
    return std::string((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}
static std::string read_bytes(const fs::path& p) { return read_text(p); }
static void write_text(const fs::path& p, const std::string& text) {
    fs::create_directories(p.parent_path());
    std::ofstream f(p, std::ios::binary);
    f.write(text.data(), (std::streamsize)text.size());
}
static void write_bytes(const fs::path& p, const std::string& text) { write_text(p, text); }
static std::vector<std::string> names_of(const fs::path& dir) {
    std::vector<std::string> out;
    for (const auto& e : fs::directory_iterator(dir)) out.push_back(e.path().filename().string());
    std::sort(out.begin(), out.end());
    return out;
}
static std::string stamp_of(std::time_t t) {
    std::tm tm{};
#ifdef _WIN32
    localtime_s(&tm, &t);
#else
    localtime_r(&t, &tm);
#endif
    char b[64];
    strftime(b, sizeof b, "%Y-%m-%d_%H-%M-%S", &tm);
    return b;
}
static std::string section(const std::string& report, const std::string& name) {
    const std::string at = "### " + name + "\n\n";
    const size_t start = report.find(at);
    if (start == std::string::npos) return "";
    const size_t from = start + at.size();
    const size_t end = report.find("\n### ", from);
    std::string out = report.substr(from, end == std::string::npos ? std::string::npos : end - from);
    while (!out.empty() && (out.back() == '\n' || out.back() == '\r')) out.pop_back();
    return out;
}
static bool has(const std::string& text, const std::string& what) { return text.find(what) != std::string::npos; }
static bool has_lower(const std::string& text, const std::string& what) {
    std::string a = text, b = what;
    for (char& c : a) c = (char)std::tolower((unsigned char)c);
    for (char& c : b) c = (char)std::tolower((unsigned char)c);
    return a.find(b) != std::string::npos;
}
static bool names_user_as_folder(const std::string& text, const std::string& user) {
    std::string lower = text;
    for (char& c : lower) c = (char)std::tolower((unsigned char)c);
    std::string name = user;
    for (char& c : name) c = (char)std::tolower((unsigned char)c);
    for (size_t i = 0; i + name.size() < lower.size(); i++)
        if ((lower[i] == '/' || lower[i] == '\\') && lower.compare(i + 1, name.size(), name) == 0 &&
            (i + 1 + name.size() >= lower.size() || !(std::isalnum((unsigned char)lower[i + 1 + name.size()]) ||
                                                      lower[i + 1 + name.size()] == '.' || lower[i + 1 + name.size()] == '_' ||
                                                      lower[i + 1 + name.size()] == '-')))
            return true;
    return false;
}
static void no_private(const fs::path& folder, const std::string& home, const std::string& user, const std::string& game) {
    for (const auto& e : fs::directory_iterator(folder)) {
        const std::string text = read_bytes(e.path());
        CHECK_SAYS(!has_lower(text, home), e.path().filename().string().c_str());
        CHECK_SAYS(!has_lower(text, game), e.path().filename().string().c_str());
        CHECK_SAYS(!names_user_as_folder(text, user), e.path().filename().string().c_str());
    }
}
static Inputs base_inputs(const fs::path& game, const fs::path& current_log, const std::string& home) {
    Inputs in;
    in.version = kVERSION;
    in.gpu = "NVIDIA GeForce RTX 4090, NVIDIA 580.95.05";
    in.options = "Upscaler: NVIDIA DLSS (default: Game's)\n";
    in.system = "13th Gen Intel(R) Core(TM) i9-13900K (32 threads), 94 GB, Arch Linux (Linux 7.2.15)";
    in.env = {"HOS_HEADLESS=1", "RX_VK_SHOTS=/tmp/shots", "RX_VK_MENU_SCRIPT=1:INS,2:ENTER", "PATH=/usr/bin", "MANGOHUD=1",
              "HOS_ENV_FILE=C:\\x\\env.txt", "SECRET_TOKEN=abc", "=C:=C:\\x", "RX_VK_SHOT_EVERY=60", "RX_VK_X_MODE=off",
              "HOS_OUTPUT_RES=3840x2160", "RX_VK_NEG=-1", "RX_VK_SCALE=1.5", "HOS_COOP_ROOM=93127", "RX_API_KEY=58213",
              "HOS_COOP_SERVER=47731", "HOS_COOP_USER=1", "HOS_COOP_PASSWORD=hunter2", "HOS_RELAY=192.168.1.5",
              "HOS_RELAY_HOST=relay.example.org:47732", "RX_VK_LONG=1234567890123"};
    in.game_dir = game.string() + "/";
    in.current_log = current_log.string();
    in.home = home;
    in.user = "alice";
#ifdef _WIN32
    in.windows = true;
#else
    in.windows = false;
#endif
    in.now = 1789000000;
    in.picture_wait_ms = 2000;
    return in;
}

// (the report's picture: as vk_replay.cpp's shot_ask, a PNG of a later present written on a thread)
static std::thread painter;
static bool ask(const char* path, uint64_t ticket) {
    painter = std::thread([p = std::string(path), ticket] {
        std::this_thread::sleep_for(std::chrono::milliseconds(30));
        write_text(p, "PNG stand-in");
        picture_written(ticket, true);
    });
    return true;
}
static std::string late_path;
static uint64_t late_ticket;
static bool ask_late(const char* path, uint64_t ticket) { late_path = path, late_ticket = ticket; return true; }
static std::atomic<bool> held{true};
static std::string last_log;
static void say(const char* line) { last_log = line; }
static bool ask_held(const char*, uint64_t) {
    while (held.load()) std::this_thread::sleep_for(std::chrono::milliseconds(5));
    return false;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    const fs::path root = argv[1];
    fs::create_directories(root);
    const fs::path home = root / "home" / "alice";
    const fs::path game = home / "Games" / "Hookshot";
    fs::create_directories(game);
    const std::string home_s = home.string(), game_s = game.string();
    const fs::path now_log = game / "hookshot_log.txt";

    write_text(now_log, "[hos host] hookshot " + std::string(kVERSION) + "\n"
                        "[hos sead::MainThread] fs: mounted content: -> " + home_s + "/Games/Hookshot/romfs\n"
                        "[hos sead::MainThread] fs: mounted save: -> " + home_s + "/Games/Hookshot/user\\save\\5a3c\n"
                        "[hos MainThread] vk: pipeline cache /run/media/alice/SD/Hookshot/mkw_vk_pipelines.bin (1 KB)\n"
                        "[hos Main] map: KokiriForest_Child (child)\n"
                        "[hos host] Malice and alice123 stay, as /x/alice123/ does\n"
                        "[hos Main] map: HyruleField_Child (child)\n");
    write_text(game / "hookshot_log.1.txt", "[hos host] hookshot v2.0.14\n"
                                            "[hos sead::MainThread] fs: mounted content: -> " + home_s +
                                                "/Games/Hookshot/romfs\n"
                                                "[hos Main] map: Market_Child (child)\n"
                                                "[hos host] CRASH: exception 0xc0000005 (read 0x0, guest 0x0) at hookshot+0x1234\n"
                                                "[hos host] CRASH: thread Main, sp 0x1000\n");

    // 1. the report's folder, its name and exactly what it holds; started from its icon, this run logs to hookshot_log.txt
    {
        Inputs in = base_inputs(game, now_log, home_s);
        in.ask_picture = ask;
        const std::string rel = write(in);
        if (painter.joinable()) painter.join();
        CHECK_SAYS(!rel.empty(), shown().c_str());
        CHECK(rel == "bug-reports/" + stamp_of(in.now));
        const fs::path folder = game / rel;
        CHECK(fs::is_directory(folder));
        CHECK(names_of(folder) == std::vector<std::string>({"hookshot_log.1.txt", "hookshot_log.txt", "picture.png", "report.txt"}));
        CHECK(names_of(game / "bug-reports") == std::vector<std::string>({folder.filename().string()}));
        no_private(folder, home_s, "alice", game_s);

        const std::string report = read_text(folder / "report.txt");
        CHECK(section(report, "Hookshot version") == kVERSION);
        CHECK(section(report, "Graphics card and driver version") == "NVIDIA GeForce RTX 4090, NVIDIA 580.95.05");
        CHECK(section(report, "CPU, RAM and operating system") == in.system);
        CHECK(section(report, "Changes from the defaults").find("Upscaler: NVIDIA DLSS (default: Game's)") == 0);
        const std::string where = section(report, "Where in the game");
        CHECK(where.find("HyruleField_Child (child)") == 0 && has(where, "Market_Child (child)"));
        CHECK(has(section(report, "hookshot_log.txt"), "hookshot v2.0.14") &&
              has(section(report, "hookshot_log.txt"), "CRASH: exception 0xc0000005"));
        CHECK(has(section(report, "Screenshots or video"), "picture.png"));
        CHECK(has(report, "Nothing else from this PC: no saves, no game files, not the options file itself."));

        // the switches: harmless values kept, everything else left out; other variables not listed
        const std::string settings = section(report, "Changes from the defaults");
        for (const char* want : {"HOS_HEADLESS=1", "MANGOHUD=1", "RX_VK_SHOT_EVERY=60", "RX_VK_X_MODE=off",
                                 "HOS_OUTPUT_RES=3840x2160", "RX_VK_NEG=-1", "RX_VK_SCALE=1.5"})
            CHECK_SAYS(has(settings, want), want);
        for (const char* left : {"RX_VK_SHOTS=(value left out)", "HOS_ENV_FILE=(value left out)",
                                 "RX_VK_MENU_SCRIPT=(value left out)", "HOS_COOP_ROOM=(value left out)",
                                 "RX_API_KEY=(value left out)", "HOS_COOP_SERVER=(value left out)",
                                 "HOS_COOP_USER=(value left out)", "HOS_COOP_PASSWORD=(value left out)",
                                 "HOS_RELAY=(value left out)", "HOS_RELAY_HOST=(value left out)", "RX_VK_LONG=(value left out)"})
            CHECK_SAYS(has(settings, left), left);
        for (const char* unwanted : {"PATH=", "SECRET_TOKEN", "=C:", "93127", "58213", "47731", "hunter2", "192.168",
                                     "relay.example", "47732", "1234567890123", "1:INS"})
            CHECK_SAYS(!has(report, unwanted), unwanted);

        // the link fills the form's fields and carries no switch value
        const size_t at = report.find("https://github.com/NotRexed/Hookshot/issues/new?template=1-game-problem.yml");
        CHECK(at != std::string::npos);
        const std::string link = report.substr(at, report.find_first_of(" \n", at) - at);
        for (const char* part : {"&version=v2.1.0-rc.3-39-gbdfe09c%20%28source%20763271ad%29",
                                 "&gpu=NVIDIA%20GeForce%20RTX%204090", "&where=HyruleField_Child%20%28child%29", "&system=",
                                 "&settings=Upscaler", "HOS_HEADLESS", "RX_VK_MENU_SCRIPT"})
            CHECK_SAYS(has(link, part), part);
        CHECK(!has(link, "%3D") && !has(link, "93127") && !has(link, "hunter2") && !has(link, "192.168") &&
              !has(link, "3840x2160") && !has(link, "1%3AINS"));
        CHECK(link.size() <= 2000);

        const std::string copy = read_text(folder / "hookshot_log.txt");
        CHECK(has(copy, "~/Games/Hookshot/romfs") && has(copy, "~/Games/Hookshot/user\\save") &&
              has(copy, "/run/media/<user>/SD/Hookshot") && has(copy, "Malice and alice123 stay, as /x/alice123/ does"));
        const std::string original = read_text(now_log);
        CHECK(std::count(copy.begin(), copy.end(), '\n') == std::count(original.begin(), original.end(), '\n'));
    }

    // 2. the menu's way: on a thread of its own, one at a time; no frame means no picture, and the report says so
    {
        Inputs in = base_inputs(game, now_log, home_s);
        in.ask_picture = ask_held;
        held = true;
        CHECK(start(in));
        CHECK(state() == kSaving);
        CHECK(!start(in));
        held = false;
        for (int i = 0; i < 3000 && state() == kSaving; i++) std::this_thread::sleep_for(std::chrono::milliseconds(5));
        CHECK(state() == kSaved);
        const fs::path folder = game / shown();
        CHECK(!fs::exists(folder / "picture.png"));
        CHECK(has(section(read_text(folder / "report.txt"), "Screenshots or video"), "No picture"));
    }

    // 3. a picture that comes after the report went without it is removed again
    {
        Inputs in = base_inputs(game, now_log, home_s);
        in.ask_picture = ask_late;
        in.picture_wait_ms = 30;
        const std::string rel = write(in);
        CHECK(!rel.empty());
        const fs::path folder = game / rel;
        CHECK(!fs::exists(folder / "picture.png"));
        write_text(late_path, "PNG stand-in");
        picture_written(late_ticket, true);
        CHECK(!fs::exists(folder / "picture.png"));
        CHECK(has(section(read_text(folder / "report.txt"), "Screenshots or video"), "No picture"));
    }

    // 4. two reports in one second get folders of their own; started from a terminal, the run before's log is the game's
    {
        const fs::path game4 = root / "terminal-game" / "Hookshot";
        fs::create_directories(game4);
        write_text(game4 / "hookshot_log.txt", "[hos host] hookshot v2.0.15\n[hos Main] map: HyruleField_Child (child)\n");
        Inputs in = base_inputs(game4, "", home_s);
        const std::string a = write(in), b = write(in);
        CHECK(!a.empty() && !b.empty() && a != b && b == a + "-2");
        const fs::path folder = game4 / a;
        CHECK(names_of(folder) == std::vector<std::string>({"hookshot_log.1.txt", "report.txt"}));
        CHECK(has(read_text(folder / "hookshot_log.1.txt"), "HyruleField_Child"));
        const std::string report = read_text(folder / "report.txt");
        CHECK(has(section(report, "Where in the game"), "terminal") && has(section(report, "hookshot_log.txt"), "terminal"));
        no_private(folder, home_s, "alice", game4.string());
    }

    // 5. a log with NUL bytes (a power cut or a hard kill): copied byte for byte, its paths redacted, the map after a NUL kept
    {
        const fs::path nul = root / "power-cut.log";
        std::string text = "[hos host] hookshot " + std::string(kVERSION) + "\n[hos Main] fs: mounted content: -> " + home_s +
                           "/Games\n[hos Main] map: KokiriForest_Child (child)\n[hos Main] half a line";
        text.push_back('\0');
        text += " [hos Main] map: LostWoods_Child (child)\n";
        text.append(4096, '\0');
        write_bytes(nul, text);
        Inputs in = base_inputs(game, nul, home_s);
        const std::string rel = write(in);
        const std::string copy = read_bytes(game / rel / "hookshot_log.txt");
        std::string expected = text;
        const size_t at = expected.find(home_s);
        if (at != std::string::npos) expected.replace(at, home_s.size(), "~");
        CHECK(copy == expected);
        CHECK(has(section(read_text(game / rel / "report.txt"), "Where in the game"), "LostWoods_Child (child)"));
    }

    // 6. a log over 10 MB keeps its first MB and its last 9 MB; the map in its middle still counts
    {
        const fs::path big = root / "big.log";
        {
            std::ofstream f(big, std::ios::binary);
            f << "[hos host] hookshot " << kVERSION << "\n";
            const std::string filler = "[hos host] vk: status: " + std::string(100, 'x') + " " + home_s + "/Games\n";
            for (int i = 0; i < 60000; i++) f << filler;
            f << "[hos Main] map: DeathMountainTrail_Child (child)\n";
            for (int i = 0; i < 60000; i++) f << filler;
            f << "[hos host] the last line\n";
        }
        Inputs in = base_inputs(game, big, home_s);
        const std::string rel = write(in);
        const fs::path copy = game / rel / "hookshot_log.txt";
        CHECK(fs::file_size(copy) <= (10u << 20) + 4096);
        const std::string text = read_text(copy);
        CHECK(text.find("[hos host] hookshot " + std::string(kVERSION)) == 0);
        CHECK(text.ends_with("[hos host] the last line\n"));
        CHECK(has(text, "of this log's middle left out here"));
        CHECK(!has(text, home_s));
        CHECK(has(section(read_text(game / rel / "report.txt"), "Where in the game"), "DeathMountainTrail_Child (child)"));
    }

    // 7. a folder that cannot be made: the system's reason
    {
        const fs::path blocked = root / "blocked" / "Hookshot";
        fs::create_directories(blocked);
        write_text(blocked / "bug-reports", "a file where the folder would go");
        Inputs in = base_inputs(blocked, now_log, home_s);
        last_log.clear();
        in.log = say;
        CHECK(write(in).empty());
        CHECK(has(last_log, "bug report not written: the folder bug-reports could not be made in the game folder"));
    }

    // 8. the redaction itself: whole folders only, either slash on Windows, whatever the case there
    CHECK(redact("-> /home/al/Games/x\n", "/home/al", "al", false) == "-> ~/Games/x\n");
    CHECK(redact("/home/al", "/home/al/", "al", false) == "~");
    CHECK(redact("/home/alice/x /home/al", "/home/al", "al", false) == "/home/alice/x ~");
    CHECK(redact("/run/media/al/SD, /var/home/al/x, user\\al\\save", "/home/al", "al", false) ==
          "/run/media/<user>/SD, /var/home/<user>/x, user\\<user>\\save");
    CHECK(redact("Malice and al, /x/alx/ and vk: al", "/home/al", "al", false) == "Malice and al, /x/alx/ and vk: al");
    CHECK(redact("/HOME/AL/x", "/home/al", "al", false) == "/HOME/AL/x");
    CHECK(redact("C:\\Users\\John Smith\\Games\\x", "C:\\Users\\John Smith", "John Smith", true) ==
          "%USERPROFILE%\\Games\\x");
    CHECK(redact("c:/users/john smith/Games", "C:\\Users\\John Smith", "John Smith", true) == "%USERPROFILE%/Games");
    CHECK(redact("D:\\Users\\JOHN SMITH\\x", "C:\\Users\\John Smith", "John Smith", true) == "D:\\Users\\<user>\\x");
    CHECK(redact("nothing here", "", "", false) == "nothing here");

    // 9. this PC's part: the system as the form asks it, and what fill_here finds
    CHECK(!system_text().empty() && has(system_text(), ", "));
    Inputs here;
    fill_here(here);
    CHECK(here.system == system_text() && here.now > 0 && !here.env.empty());

    if (fails) return 1;
    std::puts("bug report: ok (report folder, options and switches, logs redacted, NUL and long logs, picture, folder reason)");
    return 0;
}
