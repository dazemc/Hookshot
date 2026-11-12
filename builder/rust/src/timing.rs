//! Build timing: opt-in stage lines on stderr (`HOOKSHOT_BUILD_TIMING=1`) and
//! the game-free `build_times.json` a build writes into its work folder.
//!
//! The file keeps the last builds' stage seconds, unit and cache counts and
//! the same "where the time went" lines the build ends with. It holds no game
//! path and nothing read from the game: only labels, counts and clocks.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};

/// How many builds `build_times.json` keeps.
const KEEP_BUILDS: usize = 20;

pub fn enabled() -> bool {
    std::env::var_os("HOOKSHOT_BUILD_TIMING").is_some()
}

struct Active {
    kind: String,
    started: Instant,
    stages: BTreeMap<String, f64>,
    counts: BTreeMap<String, u64>,
    compile_seconds: Option<f64>,
}

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

/// Start recording this build's times. Called once at the top of a build.
pub fn begin(kind: &str) {
    ACTIVE.with(|slot| {
        *slot.borrow_mut() = Some(Active {
            kind: kind.to_owned(),
            started: Instant::now(),
            stages: BTreeMap::new(),
            counts: BTreeMap::new(),
            compile_seconds: None,
        });
    });
}

fn recording() -> bool {
    ACTIVE.with(|slot| slot.borrow().is_some())
}

fn active() -> bool {
    enabled() || recording()
}

/// Add one stage's seconds. A stage entered twice adds up, as in the Python
/// builder's PhaseClock.
pub fn record_stage(label: &str, seconds: f64) {
    ACTIVE.with(|slot| {
        if let Some(active) = slot.borrow_mut().as_mut() {
            *active.stages.entry(label.to_owned()).or_insert(0.0) += seconds.max(0.0);
        }
    });
}

/// Set a count (units, cache hits, ...). The last value wins.
pub fn count(name: &str, value: u64) {
    ACTIVE.with(|slot| {
        if let Some(active) = slot.borrow_mut().as_mut() {
            active.counts.insert(name.to_owned(), value);
        }
    });
}

/// The compile stage's own seconds, from the compiler's result.
pub fn set_compile_seconds(seconds: f64) {
    ACTIVE.with(|slot| {
        if let Some(active) = slot.borrow_mut().as_mut() {
            active.compile_seconds = Some(seconds.max(0.0));
        }
    });
}

/// The build's recorded times, with the lines it ends with.
pub struct BuildTimes {
    pub kind: String,
    pub total_seconds: f64,
    pub compile_seconds: f64,
    pub stages: BTreeMap<String, f64>,
    pub counts: BTreeMap<String, u64>,
    pub previous_seconds: Option<f64>,
}

impl BuildTimes {
    /// The build's last lines: each stage and its known parts, the compile's
    /// units and cache hits, and the total. No path and no game content.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = vec!["where the time went:".to_owned()];
        let mut seen = BTreeSet::new();
        for (label, name, prefix) in TOP_STAGES {
            let Some(seconds) = self.stages.get(*label) else {
                continue;
            };
            lines.push(row(name, 0, *seconds));
            seen.insert((*label).to_owned());
            if let Some(prefix) = prefix {
                for (child, seconds) in &self.stages {
                    if child.starts_with(prefix) && seen.insert(child.clone()) {
                        lines.push(row(&child[prefix.len()..], 1, *seconds));
                    }
                }
                // (the link and the copy are inside driver.compile: parts of
                // it, not stages beside it)
                if *label == "driver.compile" {
                    for child in ["driver.link", "driver.copy"] {
                        if let Some(seconds) = self.stages.get(child) {
                            if seen.insert(child.to_owned()) {
                                lines.push(row(&child["driver.".len()..], 1, *seconds));
                            }
                        }
                    }
                }
            }
        }
        for (label, seconds) in &self.stages {
            if seen.insert(label.clone()) {
                lines.push(row(label, 1, *seconds));
            }
        }
        if let (Some(&units), Some(&compiled), Some(&cached)) = (
            self.counts.get("units"),
            self.counts.get("game_compiled"),
            self.counts.get("game_cached"),
        ) {
            let total = compiled + cached;
            let hits = if total == 0 {
                0.0
            } else {
                100.0 * cached as f64 / total as f64
            };
            let chunks = self.counts.get("chunks").copied().unwrap_or(0);
            let parallel = self.counts.get("parallel").copied().unwrap_or(0);
            lines.push(format!(
                "compile: {chunks} chunks, {units} units, {parallel} at once: {compiled} compiled, {cached} cached ({hits:.0}% cache hits)"
            ));
        }
        if let (Some(&hits), Some(&misses)) = (
            self.counts.get("lift_cache_hits"),
            self.counts.get("lift_cache_misses"),
        ) {
            lines.push(format!("lift cache: {hits} hits, {misses} misses"));
        }
        for (prefix, name) in [
            ("host_compiled", "host"),
            ("vk_compiled", "Vulkan back end"),
        ] {
            if let (Some(&compiled), Some(&cached)) = (
                self.counts.get(prefix),
                self.counts.get(&format!("{prefix}_cached")),
            ) {
                lines.push(format!("{name}: {compiled} compiled, {cached} cached"));
            }
        }
        lines.push(match self.previous_seconds {
            Some(previous) => format!(
                "the build took {}, compiled in {} (the last one {})",
                clock(self.total_seconds),
                clock(self.compile_seconds),
                clock(previous)
            ),
            None => format!(
                "the build took {}, compiled in {}",
                clock(self.total_seconds),
                clock(self.compile_seconds)
            ),
        });
        lines
    }
}

const TOP_STAGES: &[(&str, &str, Option<&str>)] = &[
    ("driver.read", "read", None),
    ("driver.assets", "assets", None),
    ("lift.total", "lift", Some("lift.")),
    ("driver.compile", "compile", Some("compile.")),
    ("host.build", "host", None),
    ("vk.build", "Vulkan back end", None),
    ("package", "finish", None),
    ("shaders", "shaders", None),
    ("prepare", "prepare shaders", None),
];

fn row(name: &str, indent: usize, seconds: f64) -> String {
    format!("{}{:<34} {:>6.0} s", "  ".repeat(indent + 1), name, seconds)
}

fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn round1(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}

/// Take the build's times and stop recording. `None` when no build began.
pub fn finish() -> Option<BuildTimes> {
    ACTIVE.with(|slot| {
        let active = slot.borrow_mut().take()?;
        Some(BuildTimes {
            kind: active.kind,
            total_seconds: active.started.elapsed().as_secs_f64(),
            compile_seconds: active.compile_seconds.unwrap_or(0.0),
            stages: active.stages,
            counts: active.counts,
            previous_seconds: None,
        })
    })
}

/// Append this build to `<work>/build_times.json`, keeping the last
/// `KEEP_BUILDS`. Returns the previous build's total, for the build's last
/// line. The file holds only what `BuildTimes` has: no path, no game content.
pub fn write(work: &Path, times: &mut BuildTimes) -> Result<Option<f64>, String> {
    fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
    let path = work.join("build_times.json");
    let mut data = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({"schema": 1, "builds": []}));
    if data.get("schema").is_none() {
        data["schema"] = json!(1);
    }
    if !data.get("builds").is_some_and(Value::is_array) {
        data["builds"] = json!([]);
    }
    let builds = data["builds"].as_array_mut().expect("the builds array");
    let previous = builds
        .last()
        .and_then(|build| build["total_seconds"].as_f64());
    times.previous_seconds = previous;
    let summary = times.summary_lines();
    builds.push(json!({
        "when": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "kind": times.kind,
        "total_seconds": round1(times.total_seconds),
        "compile_seconds": round1(times.compile_seconds),
        "stages": times.stages.iter().map(|(name, seconds)| (name.clone(), round1(*seconds)))
            .collect::<BTreeMap<_, _>>(),
        "counts": times.counts,
        "summary": summary,
    }));
    if builds.len() > KEEP_BUILDS {
        builds.drain(0..builds.len() - KEEP_BUILDS);
    }
    let text = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(previous)
}

pub struct Stage {
    label: &'static str,
    start: Option<Instant>,
    print: bool,
}

impl Stage {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            start: active().then(Instant::now),
            print: enabled(),
        }
    }

    /// Print this stage's time now. The stage keeps running; `done` prints it
    /// again, so a long stage can be split without moving its start.
    pub fn mark(&self, what: &str) {
        if let Some(start) = self.start {
            if self.print {
                eprintln!(
                    "build timing: {:<24} {:>8.3}s  {what}",
                    self.label,
                    start.elapsed().as_secs_f64()
                );
            }
        }
    }

    pub fn done(self) {
        if let Some(start) = self.start {
            let seconds = start.elapsed().as_secs_f64();
            record_stage(self.label, seconds);
            if self.print {
                eprintln!("build timing: {:<24} {:>8.3}s", self.label, seconds);
            }
        }
    }
}

/// Time one expression, labelled by the stage around it.
pub fn span<T>(label: &'static str, body: impl FnOnce() -> T) -> T {
    let stage = Stage::new(label);
    let value = body();
    stage.done();
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hookshot-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn build_times_has_a_schema_of_stages_counts_and_the_summary() {
        let dir = scratch("build-times-schema");
        begin("build");
        record_stage("driver.read", 1.2);
        record_stage("driver.compile", 10.0);
        record_stage("compile.planning", 2.0);
        record_stage("lift.total", 4.0);
        record_stage("lift.discover", 1.5);
        count("chunks", 491);
        count("units", 812);
        count("parallel", 8);
        count("game_compiled", 100);
        count("game_cached", 712);
        count("lift_cache_hits", 294_000);
        count("lift_cache_misses", 522);
        set_compile_seconds(10.0);
        let mut times = finish().expect("a build's times");
        assert_eq!(write(&dir, &mut times).unwrap(), None);
        let text = fs::read_to_string(dir.join("build_times.json")).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema"], 1);
        let build = &value["builds"][0];
        assert!(build["when"].is_string());
        assert_eq!(build["kind"], "build");
        assert!(build["total_seconds"].is_number());
        assert_eq!(build["compile_seconds"], 10.0);
        assert_eq!(build["stages"]["driver.read"], 1.2);
        assert_eq!(build["stages"]["lift.discover"], 1.5);
        assert_eq!(build["counts"]["units"], 812);
        assert_eq!(build["counts"]["game_cached"], 712);
        assert_eq!(build["counts"]["lift_cache_hits"], 294_000);
        let summary: Vec<&str> = build["summary"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(summary[0], "where the time went:");
        assert!(summary.iter().any(|line| line.contains("812 units")));
        assert!(summary.iter().any(|line| line.contains("cache hits")));
        assert!(
            summary
                .iter()
                .any(|line| line.contains("lift cache: 294000 hits"))
        );
        assert!(summary.last().unwrap().starts_with("the build took "));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn build_times_never_records_a_game_path_or_its_content() {
        let dir = scratch("build-times-game-free");
        let marker = "GAME-CONTENT-MARKER-8f3c1d";
        fs::write(dir.join("romfs.bin"), marker).unwrap();
        fs::create_dir_all(dir.join("exefs")).unwrap();
        begin("build");
        record_stage("driver.read", 1.0);
        count("units", 1);
        let mut times = finish().unwrap();
        write(&dir, &mut times).unwrap();
        let text = fs::read_to_string(dir.join("build_times.json")).unwrap();
        assert!(!text.contains(marker), "{text}");
        assert!(!text.contains(dir.to_str().unwrap()), "{text}");
        assert!(!text.contains("romfs"), "{text}");
        assert!(!text.contains("exefs"), "{text}");
        let value: Value = serde_json::from_str(&text).unwrap();
        let build = value["builds"][0].as_object().unwrap();
        for key in build.keys() {
            assert!(
                matches!(
                    key.as_str(),
                    "when"
                        | "kind"
                        | "total_seconds"
                        | "compile_seconds"
                        | "stages"
                        | "counts"
                        | "summary"
                ),
                "unexpected build key {key}"
            );
        }
        let _ = fs::remove_dir_all(dir);
    }
}
