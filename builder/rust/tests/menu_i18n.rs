// The Insert menu's translations (gpu/vk/menu_i18n.h, vk_menu_i18n.cpp, vk_ui.h, vk_menu.cpp): every table's texts are
// the menu's, the start-up screen's or the FPS counter's, every translation keeps its printf conversions and its spaces,
// and the lookup falls back to English for a missing text or a language whose letters no font draws. Game-free: the table
// is read as text, and the real lookup is compiled with stubs for the menu's options and the game folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The C string literals of a source, adjacent ones joined, comments skipped, escapes decoded. A quote inside a comment
/// is not a string; an escaped quote is.
fn c_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == b'\'' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'\'' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if bytes[i] == b'"' {
            let mut s = String::new();
            parse_literal(text, &mut i, &mut s);
            loop {
                let save = i;
                skip_space_and_comments(text, &mut i);
                if i < bytes.len() && bytes[i] == b'"' {
                    parse_literal(text, &mut i, &mut s);
                } else {
                    i = save;
                    break;
                }
            }
            out.push(s);
            continue;
        }
        let ch = text[i..].chars().next().expect("a character");
        i += ch.len_utf8();
    }
    out
}

fn parse_literal(text: &str, i: &mut usize, s: &mut String) {
    let bytes = text.as_bytes();
    assert_eq!(bytes[*i], b'"');
    *i += 1;
    while *i < bytes.len() {
        match bytes[*i] {
            b'\\' => {
                *i += 1;
                if *i >= bytes.len() {
                    break;
                }
                s.push(match bytes[*i] {
                    b'n' => '\n',
                    b't' => '\t',
                    b'r' => '\r',
                    b'0' => '\0',
                    b'\\' => '\\',
                    b'"' => '"',
                    b'\'' => '\'',
                    other => other as char,
                });
                *i += 1;
            }
            b'"' => {
                *i += 1;
                return;
            }
            _ => {
                let ch = text[*i..].chars().next().expect("a character");
                s.push(ch);
                *i += ch.len_utf8();
            }
        }
    }
}

fn skip_space_and_comments(text: &str, i: &mut usize) {
    let bytes = text.as_bytes();
    loop {
        while *i < bytes.len() && (bytes[*i] as char).is_ascii_whitespace() {
            *i += 1;
        }
        if *i + 1 < bytes.len() && bytes[*i] == b'/' && bytes[*i + 1] == b'/' {
            *i += 2;
            while *i < bytes.len() && bytes[*i] != b'\n' {
                *i += 1;
            }
            continue;
        }
        if *i + 1 < bytes.len() && bytes[*i] == b'/' && bytes[*i + 1] == b'*' {
            *i += 2;
            while *i + 1 < bytes.len() && !(bytes[*i] == b'*' && bytes[*i + 1] == b'/') {
                *i += 1;
            }
            *i = (*i + 2).min(bytes.len());
            continue;
        }
        return;
    }
}

fn decode_c(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// The printf conversions of a text in order (as vk_menu_i18n.cpp conversion() reads them; not ' ': "80% suits" is a sign)
fn conversions(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < b.len() && matches!(b[i], b'-' | b'+' | b'#' | b'0') {
            i += 1;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        while i < b.len() && matches!(b[i], b'l' | b'h' | b'z') {
            i += 1;
        }
        if i < b.len() && matches!(b[i], b's' | b'd' | b'u' | b'c' | b'x' | b'g' | b'f' | b'%') {
            i += 1;
            out.push(s[start..i].to_string());
        }
    }
    out
}

struct Table {
    name: String,
    entries: Vec<(String, String)>,
}

struct Lang {
    code: String,
    name: String,
    table: String,
    count: usize,
}

fn catalog() -> (BTreeMap<String, Table>, Vec<Lang>) {
    let src = source("gpu/vk/menu_i18n.h");
    let table_re =
        regex::Regex::new(r"(?s)inline constexpr Entry (k\w+)\[\] = \{\n(.*?)\n\};").unwrap();
    let entry_re =
        regex::Regex::new(r#"(?m)^    \{"((?:[^"\\]|\\.)*)", "((?:[^"\\]|\\.)*)"\},$"#).unwrap();
    let lang_re =
        regex::Regex::new(r#"(?m)^    \{"([\w-]+)", "((?:[^"\\]|\\.)*)", (\w+), (\d+)\},$"#)
            .unwrap();
    let mut tables = BTreeMap::new();
    for cap in table_re.captures_iter(&src) {
        let name = cap[1].to_string();
        let body = &cap[2];
        let entries = entry_re
            .captures_iter(body)
            .map(|e| (decode_c(&e[1]), decode_c(&e[2])))
            .collect();
        tables.insert(name.clone(), Table { name, entries });
    }
    let langs = lang_re
        .captures_iter(&src)
        .map(|c| Lang {
            code: c[1].to_string(),
            name: decode_c(&c[2]),
            table: c[3].to_string(),
            count: c[4].parse().unwrap(),
        })
        .collect();
    (tables, langs)
}

/// The texts the menu draws (vk_menu.cpp) and the start-up screen and FPS counter it also draws (vk_progress.h,
/// fps_stats.h), plus the pieces vk_menu_i18n.cpp puts together (" settings (many)", "Restart needed: ").
fn drawn_texts() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for file in [
        "gpu/vk/vk_menu.cpp",
        "gpu/vk/vk_progress.h",
        "gpu/vk/fps_stats.h",
        "gpu/vk/vk_menu_i18n.cpp",
        "gpu/vk/vk_menu_i18n.h",
    ] {
        out.extend(c_strings(&source(file)));
    }
    // (a printf "%%" is the table's "%")
    let replaced: Vec<_> = out.iter().map(|s| s.replace("%%", "%")).collect();
    out.extend(replaced);
    out
}

fn table_of<'a>(tables: &'a BTreeMap<String, Table>, lang: &Lang) -> &'a Table {
    tables
        .get(&lang.table)
        .unwrap_or_else(|| panic!("{}: its table {} is missing", lang.code, lang.table))
}

#[test]
fn the_languages_are_listed_with_their_tables() {
    let (tables, langs) = catalog();
    assert_eq!(langs.len(), 14, "English and the thirteen translations");
    assert_eq!(langs[0].code, "en");
    assert_eq!(langs[0].name, "English");
    assert_eq!(langs[0].table, "nullptr");
    assert_eq!(langs[0].count, 0);
    let expected = [
        "pt-BR", "es-419", "es", "fr", "de", "it", "nl", "pl", "ru", "ja", "ko", "zh-Hans",
        "zh-Hant",
    ];
    assert_eq!(
        langs[1..]
            .iter()
            .map(|l| l.code.as_str())
            .collect::<Vec<_>>(),
        expected,
        "the menu's languages"
    );
    let codes: BTreeSet<_> = langs.iter().map(|l| l.code.as_str()).collect();
    assert_eq!(codes.len(), langs.len(), "a language listed twice");
    for lang in &langs[1..] {
        let table = table_of(&tables, lang);
        assert_eq!(
            table.entries.len(),
            lang.count,
            "{}: its count is not its table's",
            lang.code
        );
        assert!(!table.entries.is_empty(), "{}: an empty table", lang.code);
    }
}

#[test]
fn every_table_is_sorted_and_well_formed() {
    let (tables, langs) = catalog();
    for lang in &langs[1..] {
        let table = table_of(&tables, lang);
        let keys: Vec<_> = table.entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            {
                let mut sorted = keys.clone();
                sorted.sort_unstable();
                sorted
            },
            "{}: not sorted (menu_i18n.h is written sorted)",
            lang.code
        );
        assert_eq!(
            keys.iter().collect::<BTreeSet<_>>().len(),
            keys.len(),
            "{}: a text twice",
            lang.code
        );
        for (en, text) in &table.entries {
            assert!(
                !text.is_empty(),
                "{}: {en:?} has an empty translation",
                lang.code
            );
            assert_eq!(
                conversions(en),
                conversions(text),
                "{}: {en:?} and {text:?} keep different printf conversions",
                lang.code
            );
            let lead = |s: &str| s.len() - s.trim_start_matches(' ').len();
            let trail = |s: &str| s.len() - s.trim_end_matches(' ').len();
            assert_eq!(
                lead(en),
                lead(text),
                "{}: the space before {en:?}",
                lang.code
            );
            assert_eq!(
                trail(en),
                trail(text),
                "{}: the space after {en:?}",
                lang.code
            );
        }
    }
}

#[test]
fn every_key_is_a_text_the_menu_draws() {
    let (tables, _) = catalog();
    let drawn = drawn_texts();
    for table in tables.values() {
        for (en, _) in &table.entries {
            assert!(
                drawn.contains(en),
                "{}: {en:?} is no text of the menu, the start-up screen or the FPS counter (renamed? then translate it again)",
                table.name
            );
        }
    }
}

#[test]
fn the_current_controls_and_hints_are_translated_everywhere() {
    let (tables, langs) = catalog();
    let menu = c_strings(&source("gpu/vk/vk_menu.cpp"));
    let hints = [
        "Gamepad: the D-pad moves, A binds, B goes back. Ocarina note keys are only notes while the ocarina waits for one; away from it they do their other jobs.",
        "The GPU's own mesh shaders where it has them, the compute path where it has not (on Linux AMD always the compute path: RADV's mesh shaders stop the GPU, issue 2245).",
        "Those of the first controller, from its name (SDL's). With none, the keyboard's (the Switch's with XInput alone).",
    ];
    for hint in hints {
        assert!(
            menu.iter().any(|t| t == hint),
            "the menu no longer has {hint:?}"
        );
        for lang in &langs[1..] {
            let table = table_of(&tables, lang);
            assert!(
                table.entries.iter().any(|(en, _)| en == hint),
                "{}: the current hint {hint:?} has no translation",
                lang.code
            );
        }
    }
}

#[test]
fn the_startup_screen_and_the_fps_counter_are_translated_everywhere() {
    let (tables, langs) = catalog();
    let startup = c_strings(&source("gpu/vk/vk_progress.h"));
    let fps = c_strings(&source("gpu/vk/fps_stats.h"));
    let menu = c_strings(&source("gpu/vk/vk_menu.cpp"));
    let keys = [
        // (the start-up screen: vk_progress.h's strings, drawn by vk_menu.cpp draw_startup)
        ("Preparing shaders", &startup),
        ("Building pipelines", &startup),
        ("Loading pipelines", &startup),
        ("Starting the game", &startup),
        ("Closing", &startup),
        ("Reading the game's shaders", &startup),
        ("Reading the pipeline notes", &startup),
        ("Loading the shader cache", &startup),
        ("Keeping what is prepared for the next start", &startup),
        (
            "This happens once after an update. Closing the window keeps what is done.",
            &startup,
        ),
        ("Step 1 of 2", &menu),
        ("Step 2 of 2", &menu),
        ("a few seconds left", &startup),
        ("about %d s left", &startup),
        ("about %d min left", &startup),
        ("%s / %s shaders", &startup),
        ("%s / %s pipelines", &startup),
        // (the FPS counter: fps_stats.h's numbers, vk_menu.cpp's labels)
        ("game %.0f", &menu),
        ("1% low %.0f", &fps),
        ("Game", &menu),
        ("Renderer", &menu),
        ("GPU wait", &menu),
        ("Pacing", &menu),
        ("GPU time", &menu),
        ("Shaders built (4 s)", &menu),
        ("Draws left out (4 s)", &menu),
    ];
    for (key, source_texts) in keys {
        assert!(
            source_texts
                .iter()
                .any(|t| t == key || t.replace("%%", "%") == key),
            "{key:?} is no longer drawn where it was"
        );
        for lang in &langs[1..] {
            let table = table_of(&tables, lang);
            assert!(
                table.entries.iter().any(|(en, _)| en == key),
                "{}: {key:?} has no translation",
                lang.code
            );
        }
    }
}

#[test]
fn the_menu_rows_and_the_language_file_are_wired() {
    let options = source("gpu/vk/vk_menu.h");
    let menu = source("gpu/vk/vk_menu.cpp");
    let i18n = source("gpu/vk/vk_menu_i18n.cpp");
    let ui = source("gpu/vk/vk_ui.h");
    let imgui = source("gpu/vk/vk_imgui.cpp");
    let font = source("gpu/vk/vk_game_font.h");
    let progress = source("gpu/vk/vk_progress.h");

    assert!(
        options.contains("int game_lang = 0;"),
        "the game's language is not kept apart"
    );
    assert!(
        options.contains("int menu_lang = 0;"),
        "the menu's language is not kept apart"
    );
    assert!(menu.contains("\"Game language\", &opts.game_lang, 0, i18n::game_lang_count() - 1"));
    assert!(menu.contains("\"Menu language\", &opts.menu_lang, 0, i18n::lang_count()"));
    assert!(menu.contains("const char* tx_game_lang(int v)"));
    assert!(menu.contains("const char* tx_menu_lang(int v)"));
    assert!(menu.contains("const char* hn_menu_lang(int v, int variant)"));
    assert!(
        menu.contains("opts.game_lang = i18n::game_lang_index();"),
        "the row does not start at language.txt's"
    );
    assert!(
        menu.contains("} else if (r.value && r.value != &opts.game_lang) {"),
        "the options file takes the game's language"
    );
    assert!(
        menu.contains(
            "if (!r.value || r.value == &opts.game_lang || strcmp(line, ini_key(r))) continue;"
        ),
        "the options file loads the game's language instead of language.txt's"
    );
    assert!(
        menu.contains("k[n++] = *p == ' ' ? '_' : (char)tolower(*p);"),
        "the menu's language is not saved as menu_language by its row name"
    );
    assert!(
        menu.contains("void game_lang_tick()"),
        "the game's language is not written when the row is left"
    );
    assert!(
        menu.contains("game_lang_tick();"),
        "menu_tick does not write the game's language"
    );
    assert!(
        menu.contains("i18n::tr(restart_names[i])"),
        "the restart line is not translated"
    );
    assert!(menu.contains("i18n::tr(\"Restart needed: \")"));
    assert!(menu.contains("i18n::tr(\"Restart to apply \")"));

    assert!(
        i18n.contains("language.txt"),
        "the game's language is not read from language.txt"
    );
    assert!(
        i18n.contains("HOS_LANGUAGE"),
        "the game's own switch is not followed"
    );
    assert!(
        i18n.contains("set_game_language"),
        "the game's language cannot be saved"
    );
    assert!(
        i18n.contains("const std::string& game_language()"),
        "the menu's Auto has no game language"
    );
    assert!(
        i18n.contains("int lang_now()"),
        "the menu's language is not looked up"
    );
    assert!(
        i18n.contains("chosen_lang()"),
        "the chosen language is not kept apart from the drawn one"
    );
    assert!(
        i18n.contains("no_font"),
        "a script without a font does not fall back"
    );
    assert!(
        i18n.contains("return s;"),
        "a missing text does not fall back to English"
    );

    assert!(ui.contains("i18n::tr(t)"), "ui::text does not translate");
    assert!(ui.contains("inline ImVec2 size_of") && ui.contains("i18n::tr(t)"));
    assert!(ui.contains("inline float para") && ui.contains("i18n::tr(t)"));
    assert!(ui.contains("inline void spaced") && ui.contains("i18n::tr(t)"));
    assert!(
        imgui.contains("script_fonts(i18n::lang_code())"),
        "the menu's script is not merged in"
    );
    assert!(
        imgui.contains("i18n::font_missing(lang)"),
        "a missing font is not reported"
    );
    assert!(
        font.contains("read_script"),
        "the game's fonts for other scripts cannot be read"
    );
    assert!(progress.contains("\"%s / %s shaders\""));
    assert!(progress.contains("\"%s / %s pipelines\""));
    assert!(
        menu.contains("i18n::tr(fps_sub)"),
        "the frame generation line is not translated"
    );
    assert!(
        menu.contains("i18n::tr(fps_low_text)"),
        "the 1% low is not translated"
    );
    assert!(
        menu.contains("i18n::tr(name)"),
        "the Breakdown's names are not translated"
    );
    assert!(
        menu.contains("\"Step 1 of 2\"") && menu.contains("\"Step 2 of 2\""),
        "the start-up step is not translated"
    );
}

fn compiler() -> Option<String> {
    let candidates =
        std::env::var("CXX")
            .ok()
            .into_iter()
            .chain(["c++".into(), "g++".into(), "clang++".into()]);
    for candidate in candidates {
        if Command::new(&candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

const FIXTURE: &str = r#"
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include "vk_menu.h"
#include "vk_menu_i18n.h"
#include "menu_i18n.h"

namespace mkwvk {
Options opts;
}

int main(int argc, char** argv) {
    if (argc < 2) return 2;
    const std::string cmd = argv[1];
    if (cmd == "lookup") {
        mkwvk::opts.menu_lang = 2;   // (pt-BR: 1 is English, the first of kLangs)
        std::printf("%s\n", mkwvk::i18n::tr("Off"));
        std::printf("%s\n", mkwvk::i18n::tr("no such text"));
        std::printf("%s\n", mkwvk::i18n::tr("game 60"));
        std::printf("%s\n", mkwvk::i18n::tr("2 of 5"));
        std::printf("%s\n", mkwvk::i18n::tr("Restart needed: "));
        std::printf("%s\n", mkwvk::i18n::tr("12,345 / 38,809 shaders"));
        std::printf("%s\n", mkwvk::i18n::tr(""));
        return 0;
    }
    if (cmd == "lookup-lang" && argc == 4) {
        mkwvk::opts.menu_lang = std::atoi(argv[2]);
        std::printf("%s\n", mkwvk::i18n::tr(argv[3]));
        return 0;
    }
    if (cmd == "font") {
        mkwvk::opts.menu_lang = 11;   // (ja)
        std::printf("%d\n", mkwvk::i18n::lang_now());
        mkwvk::i18n::font_missing("ja");
        std::printf("%d\n", mkwvk::i18n::lang_now());
        std::printf("%s\n", mkwvk::i18n::tr("Off"));
        return 0;
    }
    if (cmd == "auto") {
        mkwvk::opts.menu_lang = 0;   // (Auto: the game's language)
        std::printf("%d\n", mkwvk::i18n::lang_now());
        std::printf("%s\n", mkwvk::i18n::tr("Off"));
        return 0;
    }
    if (cmd == "plural" && argc == 4) {
        mkwvk::opts.menu_lang = std::atoi(argv[2]);
        std::printf("%d\n", mkwvk::i18n::plural(std::atoi(argv[3])));
        return 0;
    }
    if (cmd == "language") {
        std::printf("%d\n", mkwvk::i18n::game_lang_count());
        std::printf("%s\n", mkwvk::i18n::game_lang_code(2));
        std::printf("%s\n", mkwvk::i18n::game_lang_name(2));
        std::printf("%d\n", mkwvk::i18n::game_lang_index());
        std::printf("%d\n", (int)mkwvk::i18n::set_game_language(2));
        std::printf("%d\n", mkwvk::i18n::game_lang_index());
        const char* dir = std::getenv("I18N_TEST_DIR");
        const std::string path = std::string(dir ? dir : ".") + "/language.txt";
        if (FILE* f = std::fopen(path.c_str(), "r")) {
            char line[256];
            while (std::fgets(line, sizeof line, f)) std::printf("%s", line);
            std::fclose(f);
        }
        return 0;
    }
    return 2;
}
"#;

const STUB_MENU: &str = r#"
#pragma once
namespace mkwvk {
struct Options {
    int menu_lang = 0;
};
extern Options opts;
}
"#;

const STUB_PATHS: &str = r#"
#pragma once
#include <cstdlib>
#include <cstring>
#include <string>
using DWORD = unsigned long;
#ifndef MAX_PATH
#define MAX_PATH 4096
#endif
namespace mkwvk {
inline DWORD game_module_path(char* buf, DWORD size) {
    const char* dir = std::getenv("I18N_TEST_DIR");
    const std::string path = std::string(dir && *dir ? dir : ".") + "/hookshot.exe";
    if (path.size() + 1 > size) return 0;
    std::memcpy(buf, path.c_str(), path.size() + 1);
    return (DWORD)path.size();
}
}
"#;

fn build_fixture() -> (PathBuf, PathBuf) {
    let compiler = compiler().expect("a C++ compiler for the lookup fixture");
    let unique = format!(
        "hookshot-menu-i18n-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    let stub = dir.join("stub");
    fs::create_dir_all(&stub).expect("the fixture's folder");
    fs::write(dir.join("main.cpp"), FIXTURE).unwrap();
    fs::write(stub.join("vk_menu.h"), STUB_MENU).unwrap();
    fs::write(stub.join("vk_paths.h"), STUB_PATHS).unwrap();
    // (the real lookup, copied so its quote includes find the stubs before gpu/vk's own vk_menu.h)
    fs::copy(
        root().join("gpu/vk/vk_menu_i18n.cpp"),
        dir.join("vk_menu_i18n.cpp"),
    )
    .unwrap();
    let exe = dir.join("i18n-test");
    let status = Command::new(&compiler)
        .args(["-std=c++20", "-O0", "-I"])
        .arg(&stub)
        .arg("-I")
        .arg(root().join("gpu/vk"))
        .arg(dir.join("main.cpp"))
        .arg(dir.join("vk_menu_i18n.cpp"))
        .arg("-o")
        .arg(&exe)
        .status()
        .expect("the fixture's compiler");
    assert!(status.success(), "the lookup fixture does not compile");
    (dir, exe)
}

fn run_env(exe: &Path, dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
    let mut command = Command::new(exe);
    command.args(args).env("I18N_TEST_DIR", dir);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().expect("the lookup fixture runs");
    assert!(
        output.status.success(),
        "the lookup fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 output")
}

fn run(exe: &Path, dir: &Path, args: &[&str]) -> String {
    run_env(exe, dir, args, &[])
}

#[test]
fn the_lookup_translates_formats_and_falls_back_to_english() {
    let (dir, exe) = build_fixture();
    let out = run(&exe, &dir, &["lookup"]);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "Desligado", "an exact text");
    assert_eq!(lines[1], "no such text", "a missing text stays English");
    assert_eq!(lines[2], "jogo 60", "a printf format is translated");
    assert_eq!(lines[3], "2 de 5", "a format with two numbers");
    assert_eq!(lines[4], "Requer reinício: ", "the restart line's piece");
    assert_eq!(
        lines[5], "12,345 / 38,809 shaders",
        "the start-up screen's count line"
    );
    assert_eq!(lines[6], "", "an empty text");

    let (tables, langs) = catalog();
    let es_419 = langs.iter().find(|l| l.code == "es-419").unwrap();
    let table = table_of(&tables, es_419);
    assert!(
        !table.entries.iter().any(|(key, _)| key == "Audio"),
        "Spanish (Latin America) now has Audio: pick another missing key"
    );
    let out = run(&exe, &dir, &["lookup-lang", "3", "Audio"]);
    assert_eq!(
        out.trim_end(),
        "Audio",
        "a missing key falls back to English"
    );
    let out = run(&exe, &dir, &["lookup-lang", "2", "Off"]);
    assert_eq!(out.trim_end(), "Desligado", "a key the table has");
    fs::remove_dir_all(dir).ok();
}

#[test]
fn auto_follows_the_game_language_and_the_plural_forms() {
    let (dir, exe) = build_fixture();
    // (fr-CA takes French: its first part, as the Auto rule says)
    fs::write(dir.join("language.txt"), "fr-CA\n").unwrap();
    let out = run(&exe, &dir, &["auto"]);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "4", "French is the menu's language");
    assert_eq!(lines[1], "Désactivé", "and its text is French");

    // (HOS_LANGUAGE wins over language.txt, any case, as the game's own start reads it)
    fs::remove_file(dir.join("language.txt")).unwrap();
    let out = run_env(&exe, &dir, &["auto"], &[("HOS_LANGUAGE", "pt-br")]);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "1", "Portuguese (Brazil) from the game's switch");
    assert_eq!(lines[1], "Desligado");

    // (the restart line's plural forms: ru 1/2-4/5+, pl 1/2-4/5+; menu_lang 10 is ru, 9 is pl)
    for (n, want) in [("1", "0"), ("2", "1"), ("5", "2"), ("11", "2"), ("21", "0")] {
        assert_eq!(
            run(&exe, &dir, &["plural", "10", n]).trim_end(),
            want,
            "ru plural {n}"
        );
    }
    for (n, want) in [("1", "0"), ("2", "1"), ("5", "2"), ("12", "2"), ("22", "1")] {
        assert_eq!(
            run(&exe, &dir, &["plural", "9", n]).trim_end(),
            want,
            "pl plural {n}"
        );
    }
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_language_without_a_font_falls_back_to_english() {
    let (dir, exe) = build_fixture();
    let out = run(&exe, &dir, &["font"]);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "10", "Japanese is chosen");
    assert_eq!(lines[1], "0", "without a font it is drawn in English");
    assert_eq!(lines[2], "Off", "and its texts are English");
    fs::remove_dir_all(dir).ok();
}

#[test]
fn the_game_language_is_read_and_written_apart_from_the_menu() {
    let (dir, exe) = build_fixture();
    fs::write(dir.join("language.txt"), "# the game's language\nen-US\n").unwrap();
    let out = run(&exe, &dir, &["language"]);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines[0], "16", "the game's languages");
    assert_eq!(lines[1], "fr", "the third is French");
    assert_eq!(lines[2], "Français (France)");
    assert_eq!(lines[3], "0", "language.txt says English (US)");
    assert_eq!(lines[4], "1", "the write succeeds");
    assert_eq!(lines[5], "2", "the saved language is now French");
    assert_eq!(lines[6], "# the game's language", "the comment stays");
    assert_eq!(lines[7], "fr", "the code line is replaced");
    fs::remove_dir_all(dir).ok();
}
