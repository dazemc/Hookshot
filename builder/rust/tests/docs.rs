// Game-free checks of the documentation: every relative Markdown link's file target exists, and the contributor docs
// keep the app on the Rust/Flutter path. No game, network or external tool is involved.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    let path = root().join(path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The repository's Markdown files. Vendored trees are left out: their links are upstream's, not ours.
fn markdown_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(&root(), &mut files);
    files.sort();
    files
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if matches!(
                name.as_str(),
                ".git"
                    | ".dart_tool"
                    | "_deps"
                    | "build"
                    | "build-linux"
                    | "node_modules"
                    | "target"
                    | "third_party"
                    | "work"
            ) {
                continue;
            }
            walk(&path, files);
        } else if name.ends_with(".md") || name.ends_with(".markdown") {
            files.push(path);
        }
    }
}

/// The inline link and image targets on one line, outside inline code spans.
fn link_targets(line: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut code_ticks = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let mut run = 1;
            while i + run < bytes.len() && bytes[i + run] == b'`' {
                run += 1;
            }
            if code_ticks == 0 {
                code_ticks = run;
            } else if code_ticks == run {
                code_ticks = 0;
            }
            i += run;
            continue;
        }
        if code_ticks == 0 && bytes[i] == b'[' {
            let Some(close) = line[i + 1..].find("](") else {
                i += 1;
                continue;
            };
            let close = i + 1 + close;
            let Some(end) = line[close + 2..].find(')') else {
                i += 1;
                continue;
            };
            targets.push(line[close + 2..close + 2 + end].trim().to_owned());
            i = close + 2 + end + 1;
            continue;
        }
        i += 1;
    }
    targets
}

/// `[text](<path with spaces> "title")` and `[text](path "title")` both name `path`.
fn target_path(target: &str) -> &str {
    let target = target.trim();
    if let Some(rest) = target.strip_prefix('<') {
        return rest.split('>').next().unwrap_or(rest);
    }
    target.split_whitespace().next().unwrap_or(target)
}

fn is_external(target: &str) -> bool {
    target.starts_with('#')
        || target.starts_with('/')
        || target.contains("://")
        || target.starts_with("mailto:")
        || target.starts_with("data:")
        || target.starts_with("tel:")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex = [bytes[i + 1], bytes[i + 2]];
            let hex = std::str::from_utf8(&hex).unwrap_or_default();
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// GitHub-style heading slug. Lower-cases, turns whitespace into single hyphens and drops
/// punctuation, matching the anchors GitHub generates for a Markdown heading.
fn slugify(heading: &str) -> String {
    // strip common inline formatting
    let mut text = heading.to_string();
    // remove link/image targets, keep link text
    // naive: replace [text](url) -> text
    // handle bold/italic/code by dropping the markers
    // simple pass: drop backticks and asterisks/underscores used as emphasis
    text = text.replace('`', "");
    // keep it simple: remove markdown link/image syntax -> text
    // Use a tiny state machine is overkill; do regex-ish replacements manually.
    // Replace [..](..) -> inner text
    let mut out = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // find matching ](
            if let Some(close) = text[i..].find("](") {
                let close = i + close;
                // link text is text[i+1..close]
                out.push_str(&text[i + 1..close]);
                // skip to matching ')'
                if let Some(end) = text[close + 2..].find(')') {
                    i = close + 2 + end + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    text = out;

    // lowercase
    let mut slug = String::new();
    let mut prev_hyphen = true;
    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '_' {
            slug.push(ch);
            prev_hyphen = false;
        } else if ch.is_whitespace() || ch == '-' {
            if !prev_hyphen {
                slug.push('-');
                prev_hyphen = true;
            }
        }
        // drop other punctuation
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Collect the set of anchors GitHub would generate for the headings of a Markdown file.
fn heading_anchors(text: &str) -> std::collections::HashSet<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut anchors = std::collections::HashSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }
        // count leading #
        let level = trimmed.chars().take_while(|c| *c == '#').count();
        if level == 0 || level > 6 {
            continue;
        }
        let rest = trimmed[level..].trim_start();
        if rest.is_empty() {
            continue;
        }
        // strip trailing # (atx closing)
        let heading = rest.trim_end_matches('#').trim();
        if heading.is_empty() {
            continue;
        }
        let base = slugify(heading);
        if base.is_empty() {
            continue;
        }
        let n = counts.entry(base.clone()).or_insert(0);
        let anchor = if *n == 0 {
            base.clone()
        } else {
            format!("{}-{}", base, n)
        };
        *n += 1;
        anchors.insert(anchor);
    }
    anchors
}

// Allowlist: anchors that are generated at runtime or otherwise not present as static headings.
const ANCHOR_ALLOWLIST: &[&str] = &[];

#[test]
fn relative_markdown_links_resolve() {
    let files = markdown_files();
    assert!(
        files.len() >= 20,
        "only {} Markdown files found: the walk from {} missed the repository",
        files.len(),
        root().display()
    );
    let mut broken = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let mut fence: Option<&str> = None;
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if let Some(marker) = fence {
                if trimmed.starts_with(marker) {
                    fence = None;
                }
                continue;
            }
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fence = Some(if trimmed.starts_with("```") {
                    "```"
                } else {
                    "~~~"
                });
                continue;
            }
            for target in link_targets(line) {
                let target = target_path(&target);
                if is_external(target) {
                    continue;
                }
                let path = target.split(['#', '?']).next().unwrap_or(target);
                if path.is_empty() {
                    continue;
                }
                let path = percent_decode(path);
                let destination = file.parent().unwrap_or(Path::new(".")).join(&path);
                if !destination.exists() {
                    let relative = file.strip_prefix(root()).unwrap_or(file);
                    broken.push(format!(
                        "{}:{}: {target} -> {}",
                        relative.display(),
                        number + 1,
                        destination.display()
                    ));
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "relative Markdown links with no target:\n{}",
        broken.join("\n")
    );
}

#[test]
fn relative_markdown_anchors_resolve() {
    let files = markdown_files();
    let mut broken = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let mut fence: Option<&str> = None;
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if let Some(marker) = fence {
                if trimmed.starts_with(marker) {
                    fence = None;
                }
                continue;
            }
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fence = Some(if trimmed.starts_with("```") { "```" } else { "~~~" });
                continue;
            }
            for target in link_targets(line) {
                let target = target_path(&target);
                if is_external(target) {
                    // external links with fragments to other sites are not our concern;
                    // but same-page anchors (#foo) are handled by is_external returning true for '#'-prefixed,
                    // so skip them here.
                    continue;
                }
                let (path, fragment) = match target.split_once('#') {
                    Some((p, f)) => (p, f),
                    None => continue,
                };
                if fragment.is_empty() {
                    continue;
                }
                let path = percent_decode(path);
                let fragment = percent_decode(fragment);
                let fragment = fragment.trim_start_matches('#').to_string();
                if ANCHOR_ALLOWLIST.contains(&fragment.as_str()) {
                    continue;
                }
                // resolve destination
                let destination = if path.is_empty() {
                    file.clone()
                } else {
                    file.parent().unwrap_or(Path::new(".")).join(&path)
                };
                if !destination.exists() {
                    continue; // file-level check covers this
                }
                let dest_text = fs::read_to_string(&destination).unwrap_or_default();
                let anchors = heading_anchors(&dest_text);
                let frag_slug = slugify(&fragment);
                // GitHub compares the raw fragment too; accept either raw or slugified
                if !anchors.contains(&fragment) && !anchors.contains(&frag_slug) {
                    let relative = file.strip_prefix(root()).unwrap_or(file);
                    broken.push(format!(
                        "{}:{}: {}#{} -> anchor not found (slug: {})",
                        relative.display(),
                        number + 1,
                        path,
                        fragment,
                        frag_slug
                    ));
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "relative Markdown links with broken anchors:\n{}",
        broken.join("\n")
    );
}

#[test]
fn contributor_docs_keep_the_app_on_the_rust_path() {
    // (the docs a contributor reads before building or sending a change)
    let docs = [
        "CONTRIBUTING.md",
        "docs/good-first-issues.md",
        "readme.md",
        "AGENTS.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
    ];
    let stale = [
        "python -I nx2/",
        "python -m pip install -r requirements",
        "pip install -r requirements",
        "python -I scripts/ngx_sdk.py",
        "Run everything with `python -I`",
        "Install Python 3",
        "make_game.py --nup",
        "make_game.py --romfs",
    ];
    for doc in docs {
        let text = source(doc);
        for needle in stale {
            assert!(
                !text.contains(needle),
                "{doc} still sends contributors to the Python app: {needle}"
            );
        }
    }

    let contributing = source("CONTRIBUTING.md");
    for needle in [
        "cargo test --workspace",
        "flutter test",
        "builder/rust",
        "builder/flutter",
        "dazemc/Hookshot-rs",
    ] {
        assert!(
            contributing.contains(needle),
            "CONTRIBUTING.md no longer names the current contributor path: {needle}"
        );
    }
    assert!(
        !contributing.contains("github.com/NotRexed/Hookshot/issues")
            && !contributing.contains("github.com/NotRexed/Hookshot/releases"),
        "CONTRIBUTING.md sends issues and releases to upstream; this fork uses dazemc/Hookshot-rs"
    );
}
