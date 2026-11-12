// The Insert menu's sections (gpu/vk/vk_menu.cpp, upstream NotRexed/Hookshot eaef1460): a heading row groups a tab's rows,
// the selection steps over it, and no section can be left empty on a PC without one of its rows. The Graphics tab's sections
// after the Quality preset are checked by name and member, and the menu restart fixture (tests/test_menu_restart.cpp) is
// compiled and run. Game-free: the sources are read and one header-only fixture runs; no game, dump or GPU is needed.

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

/// A C++ definition's braces, matched, from its first line.
fn block(text: &str, marker: &str) -> String {
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + text[start..].find('{').expect("opening brace") + 1;
    let bytes = text.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    text[start..end].to_string()
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

#[derive(Debug)]
struct Row {
    tab: String,
    name: String,
    value: String,
    heading: bool,
}

fn rows(menu: &str) -> Vec<Row> {
    let start = menu.find("Row rows[] = {").expect("the rows table");
    let end = start + menu[start..].find("\n};").expect("the rows table's end");
    let table = &menu[start..end];
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(offset) = table[at..].find("\n    {TAB_") {
        let from = at + offset + 1;
        let to = table[from..]
            .find("\n    {TAB_")
            .map(|next| from + next)
            .unwrap_or(table.len());
        let entry = &table[from..to];
        let first = entry.lines().next().expect("a row's first line").trim();
        let mut parts = first.splitn(3, ", ");
        let tab = parts
            .next()
            .and_then(|part| part.strip_prefix('{'))
            .expect("a row's tab");
        let name = parts
            .next()
            .and_then(|part| part.strip_prefix('"'))
            .and_then(|part| part.strip_suffix('"'))
            .expect("a row's name");
        let value = parts.next().expect("a row's value").trim();
        out.push(Row {
            tab: tab.to_owned(),
            name: name.to_owned(),
            value: value.to_owned(),
            heading: entry.contains(
                "nullptr, 0, 0, 0, nullptr, nullptr, nullptr, false, true, -1, true}",
            ),
        });
        at = to;
    }
    out
}

const GRAPHICS: &[(&str, &[&str])] = &[
    (
        "Resolution and upscaling",
        &[
            "Aspect ratio",
            "Output resolution",
            "Profile",
            "Upscaler",
            "DLSS model",
            "Render resolution",
            "Sharpness",
        ],
    ),
    (
        "Frame generation and latency",
        &[
            "Frame generation",
            "Frame generation by",
            "Frame pacing",
            "Game frame rate cap",
            "NVIDIA Reflex",
            "AMD Anti-Lag 2",
        ],
    ),
    (
        "Screen",
        &[
            "Fullscreen",
            "V-Sync",
            "Window scaling filter",
            "Ultrawide menus",
        ],
    ),
    (
        "HDR",
        &[
            "HDR",
            "HDR paper white",
            "HDR peak brightness",
            "HDR contrast",
            "HDR tone mapper",
        ],
    ),
    ("Detail", &["Texture detail", "Anisotropic filtering"]),
    ("GPU and memory", &["GPU device", "VRAM budget"]),
];

#[test]
fn every_graphics_section_has_a_row_this_pc_can_show() {
    let menu = source("gpu/vk/vk_menu.cpp");
    let rows = rows(&menu);
    assert!(rows.len() >= 40, "the rows table has {} rows", rows.len());

    // The values row_shown can hide: a section may not be made only of them.
    let shown = block(&menu, "bool row_shown(int i) {");
    let mut hidable = Vec::new();
    let mut rest = shown.as_str();
    while let Some(at) = rest.find("&opts.") {
        let after = &rest[at + 6..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let length = name.len();
        if !name.is_empty() {
            hidable.push(name);
        }
        rest = &after[length..];
    }
    assert!(
        hidable.iter().any(|name| name == "reflex")
            && hidable.iter().any(|name| name == "hdr_white")
            && hidable.iter().any(|name| name == "hdr_tonemap"),
        "row_shown no longer reads the rows the sections rely on: {hidable:?}"
    );

    let mut tabs: Vec<(String, Vec<&Row>)> = Vec::new();
    for row in &rows {
        match tabs.iter_mut().find(|(tab, _)| tab == &row.tab) {
            Some((_, list)) => list.push(row),
            None => tabs.push((row.tab.clone(), vec![row])),
        }
    }
    for (tab, entries) in &tabs {
        for (i, row) in entries.iter().enumerate() {
            if !row.heading {
                continue;
            }
            let mut section = Vec::new();
            for later in &entries[i + 1..] {
                if later.heading {
                    break;
                }
                section.push(later);
            }
            assert!(
                !section.is_empty(),
                "{tab}: the section {:?} has no rows",
                row.name
            );
            let all_hidden = section.iter().all(|later| {
                later
                    .value
                    .strip_prefix("&opts.")
                    .is_some_and(|name| hidable.iter().any(|hidden| hidden == name))
            });
            assert!(
                !all_hidden,
                "{tab}: every row of the section {:?} can be hidden by row_shown: a PC without them would show a title over nothing",
                row.name
            );
        }
    }

    // The Graphics tab starts with the Quality preset, which sets rows in every section, then its headings in this order.
    let graphics: Vec<&Row> = rows
        .iter()
        .filter(|row| row.tab == "TAB_GRAPHICS")
        .collect();
    assert_eq!(graphics[0].name, "Quality preset");
    assert!(!graphics[0].heading);
    let titles: Vec<&str> = graphics
        .iter()
        .filter(|row| row.heading)
        .map(|row| row.name.as_str())
        .collect();
    let wanted: Vec<&str> = GRAPHICS.iter().map(|(title, _)| *title).collect();
    assert_eq!(titles, wanted, "the Graphics tab's sections");

    let mut current = None;
    let mut section_of = std::collections::BTreeMap::new();
    for row in &graphics {
        if row.heading {
            current = Some(row.name.as_str());
        } else if let Some(title) = current {
            section_of.insert(row.name.as_str(), title);
        }
    }
    for (title, members) in GRAPHICS {
        for member in *members {
            assert_eq!(
                section_of.get(member).copied(),
                Some(*title),
                "{member:?} is not under {title:?}"
            );
        }
    }

    // A heading is drawn as a title, stepped over by the keys, and is not a row the bridge sets.
    assert!(block(&menu, "void draw_options(ui::Ctx& c) {").contains("if (r.heading) {"));
    let tick = block(&menu, "void menu_tick() {");
    assert!(tick.contains("while (heading_at(selected));"));
    assert!(tick.contains("auto heading_at = [&](int k) { return rows[tab_row_to_global(cur_tab, k)].heading; };"));
    assert!(menu.contains("bool bridge_row(int i) { return rows[i].value && !rows[i].heading"));

    let Some(cxx) = compiler() else {
        eprintln!("skipping the menu restart fixture: no C++ compiler (CXX, c++, g++ or clang++)");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "hookshot-menu-sections-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    let exe = dir.join(if cfg!(windows) { "restart.exe" } else { "restart" });
    let build = Command::new(&cxx)
        .args(["-std=c++20", "-Wall", "-Wextra", "-Werror"])
        .arg(root().join("tests/test_menu_restart.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .expect("run the C++ compiler");
    assert!(
        build.status.success(),
        "{cxx}: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&exe).output().expect("run the menu restart fixture");
    let _ = fs::remove_dir_all(dir);
    assert!(
        run.status.success(),
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).contains("menu restart: all checks passed"));
}
