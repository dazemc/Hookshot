//! The build's timing report: read a `build_times.json`, show where a build's
//! seconds went, and compare a run against a baseline with tolerances.
//!
//! ```text
//! cargo run --bin build_times -- table <work>/build_times.json
//! cargo run --bin build_times -- compare <work>/build_times.json <baseline.json> \
//!     --json build-times.json --markdown build-times.md
//! ```
//!
//! The tool reads the schema the Rust builder writes (`schema: 1`, `builds[]`
//! with `stages`) and the earlier Python record (`format: 1`, `builds[]` with
//! `phases`). It prints only stage names, seconds and shares: no `when`, no
//! game path and no game content. A comparison exits 1 when a stage is over
//! its baseline tolerance (or a baseline stage is missing), so a build job can
//! use it as a regression gate.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// The only file schema this tool reads. `format: 1` is the Python record's
/// number for the same shape of stages.
pub const SCHEMA: u64 = 1;
/// A stage may grow by this share of its baseline before it is a regression.
pub const DEFAULT_PERCENT: f64 = 10.0;
/// ...plus this many seconds, so a short stage is not all noise.
pub const DEFAULT_SECONDS: f64 = 2.0;

/// A canonical stage. The order of the variants is the table's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Checks,
    Read,
    Lift,
    Compile,
    Planning,
    Compiling,
    Links,
    Finish,
    Host,
    Vulkan,
    Assets,
    Standalone,
    Shaders,
    Prepare,
    Other,
}

impl Stage {
    /// The rows every table shows, in the benchmark's order.
    pub const CORE: [Stage; 8] = [
        Stage::Checks,
        Stage::Read,
        Stage::Lift,
        Stage::Compile,
        Stage::Planning,
        Stage::Compiling,
        Stage::Links,
        Stage::Finish,
    ];
    /// Rows shown only when the record has them.
    pub const EXTRA: [Stage; 7] = [
        Stage::Host,
        Stage::Vulkan,
        Stage::Assets,
        Stage::Standalone,
        Stage::Shaders,
        Stage::Prepare,
        Stage::Other,
    ];

    /// The stable name used in JSON, Markdown and baseline files.
    pub fn key(self) -> &'static str {
        match self {
            Stage::Checks => "checks",
            Stage::Read => "read",
            Stage::Lift => "lift",
            Stage::Compile => "compile",
            Stage::Planning => "compile.planning",
            Stage::Compiling => "compile.compiling",
            Stage::Links => "links",
            Stage::Finish => "finish",
            Stage::Host => "host",
            Stage::Vulkan => "vulkan",
            Stage::Assets => "assets",
            Stage::Standalone => "standalone",
            Stage::Shaders => "shaders",
            Stage::Prepare => "prepare",
            Stage::Other => "other",
        }
    }

    /// The table's label; the compile's parts and the links sit under it.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Planning => "  planning",
            Stage::Compiling => "  compiling",
            Stage::Links => "  links",
            Stage::Vulkan => "Vulkan back end",
            Stage::Prepare => "prepare shaders",
            _ => self.key(),
        }
    }
}

/// The stage a label in either record means. `None` is a label the record's
/// own version added; it is summed into `other` and never printed by name.
pub fn stage_of(name: &str) -> Option<Stage> {
    match name {
        "checks" => Some(Stage::Checks),
        "read" | "driver.read" => Some(Stage::Read),
        "lift" | "lift.total" => Some(Stage::Lift),
        "compile" | "driver.compile" => Some(Stage::Compile),
        "compile.planning" | "compile: planning" => Some(Stage::Planning),
        "compile.compiling" | "compile: compiling" => Some(Stage::Compiling),
        "links" | "driver.link" | "driver.copy" => Some(Stage::Links),
        "finish" | "package" => Some(Stage::Finish),
        "host" | "host.build" => Some(Stage::Host),
        "vulkan" | "vk" | "vk.build" => Some(Stage::Vulkan),
        "assets" | "driver.assets" => Some(Stage::Assets),
        "standalone" | "driver.standalone" => Some(Stage::Standalone),
        "shaders" => Some(Stage::Shaders),
        "prepare" | "prepare shaders" => Some(Stage::Prepare),
        "other" => Some(Stage::Other),
        _ if name.starts_with("compile: linking ") => Some(Stage::Links),
        _ => None,
    }
}

/// Turn a record's labels into the canonical stages. A record with a total
/// uses it; one with only parts adds them up. The lift's own parts are folded
/// into its total, and an unknown label's seconds are kept in `other`.
pub fn normalize_stages(raw: &BTreeMap<String, f64>) -> BTreeMap<Stage, f64> {
    let mut stages = BTreeMap::new();
    let mut explicit = BTreeSet::new();
    let mut lift_parts = 0.0;
    let mut unknown = 0.0;
    for (name, seconds) in raw {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        if name != "lift.total" && name.starts_with("lift.") {
            lift_parts += seconds;
            continue;
        }
        match stage_of(name) {
            Some(stage) => {
                *stages.entry(stage).or_insert(0.0) += seconds;
                if matches!(stage, Stage::Lift | Stage::Compile) {
                    explicit.insert(stage);
                }
            }
            None => unknown += seconds,
        }
    }
    if !explicit.contains(&Stage::Lift) && lift_parts > 0.0 {
        stages.insert(Stage::Lift, lift_parts);
    }
    if !explicit.contains(&Stage::Compile) {
        let parts: f64 = [Stage::Planning, Stage::Compiling, Stage::Links]
            .iter()
            .filter_map(|stage| stages.get(stage))
            .sum();
        if parts > 0.0 {
            stages.insert(Stage::Compile, parts);
        }
    }
    if unknown > 0.0 {
        *stages.entry(Stage::Other).or_insert(0.0) += unknown;
    }
    stages
}

/// One build in a `build_times.json`.
#[derive(Clone, Debug)]
pub struct Build {
    pub total_seconds: f64,
    pub compile_seconds: f64,
    pub stages: BTreeMap<Stage, f64>,
}

/// A `build_times.json`'s kept builds, oldest first.
#[derive(Clone, Debug)]
pub struct TimesFile {
    pub builds: Vec<Build>,
}

/// An authored baseline: canonical (or raw) stage seconds and the tolerance a
/// run is held to. It is not a build record, so it carries no build date.
#[derive(Clone, Debug)]
pub struct BaselineFile {
    pub total_seconds: Option<f64>,
    pub stages: BTreeMap<Stage, f64>,
    pub tolerance: Option<Tolerance>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    pub percent: f64,
    pub seconds: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            percent: DEFAULT_PERCENT,
            seconds: DEFAULT_SECONDS,
        }
    }
}

impl Tolerance {
    /// The largest run time that is still inside the tolerance.
    pub fn limit(self, baseline: f64) -> f64 {
        baseline * (1.0 + self.percent / 100.0) + self.seconds
    }
}

/// Which build of a file to read. `One` is 1-based, as a person counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Select {
    Latest,
    One(usize),
}

impl Select {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text == "latest" {
            return Ok(Select::Latest);
        }
        let number: usize = text
            .parse()
            .map_err(|_| format!("{text:?} is not a build number or \"latest\""))?;
        if number == 0 {
            return Err("a build number starts at 1".to_owned());
        }
        Ok(Select::One(number))
    }
}

#[derive(Clone, Debug)]
pub enum Parsed {
    Times(TimesFile),
    Baseline(BaselineFile),
}

fn number(value: Option<&Value>, what: &str) -> Result<Option<f64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let number = value
        .as_f64()
        .ok_or_else(|| format!("{what} is not a number"))?;
    if !number.is_finite() || number < 0.0 {
        return Err(format!("{what} is not a non-negative number"));
    }
    Ok(Some(number))
}

fn stage_map(object: &Map<String, Value>) -> Result<BTreeMap<String, f64>, String> {
    let mut stages = BTreeMap::new();
    // (the Python record calls them phases; the Rust one calls them stages)
    for key in ["phases", "stages"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let map = value
            .as_object()
            .ok_or_else(|| format!("{key} is not an object"))?;
        for (name, value) in map {
            let seconds = value
                .as_f64()
                .ok_or_else(|| format!("stage {name:?} is not a number"))?;
            if !seconds.is_finite() || seconds < 0.0 {
                return Err(format!("stage {name:?} is not a non-negative number"));
            }
            stages.insert(name.clone(), seconds);
        }
    }
    Ok(stages)
}

fn parse_tolerance(value: &Value) -> Result<Tolerance, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "tolerance is not an object".to_owned())?;
    Ok(Tolerance {
        percent: number(object.get("percent"), "tolerance.percent")?.unwrap_or(DEFAULT_PERCENT),
        seconds: number(object.get("seconds"), "tolerance.seconds")?.unwrap_or(DEFAULT_SECONDS),
    })
}

fn parse_baseline(object: &Map<String, Value>) -> Result<BaselineFile, String> {
    match object.get("schema") {
        Some(schema) if schema.as_u64() != Some(SCHEMA) => {
            return Err(format!(
                "unsupported baseline schema {schema}; this tool reads schema {SCHEMA}"
            ));
        }
        None => return Err("a baseline needs \"schema\": 1".to_owned()),
        _ => {}
    }
    let raw = stage_map(object)?;
    if raw.is_empty() {
        return Err("a baseline has no stages".to_owned());
    }
    Ok(BaselineFile {
        total_seconds: number(object.get("total_seconds"), "total_seconds")?,
        stages: normalize_stages(&raw),
        tolerance: object.get("tolerance").map(parse_tolerance).transpose()?,
    })
}

fn parse_build(value: &Value, index: usize) -> Result<Build, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("build {} is not an object", index + 1))?;
    let total_seconds = number(object.get("total_seconds"), "total_seconds")?
        .ok_or_else(|| format!("build {} has no total_seconds", index + 1))?;
    let compile_seconds = number(object.get("compile_seconds"), "compile_seconds")?.unwrap_or(0.0);
    let raw = stage_map(object)?;
    Ok(Build {
        total_seconds,
        compile_seconds,
        stages: normalize_stages(&raw),
    })
}

/// Read either a `build_times.json` or an authored baseline.
pub fn parse(text: &str) -> Result<Parsed, String> {
    let root: Value = serde_json::from_str(text).map_err(|error| format!("not JSON: {error}"))?;
    let object = root
        .as_object()
        .ok_or_else(|| "the file is not a JSON object".to_owned())?;
    if object.get("baseline").and_then(Value::as_bool) == Some(true) {
        return Ok(Parsed::Baseline(parse_baseline(object)?));
    }
    match object.get("schema") {
        Some(schema) if schema.as_u64() != Some(SCHEMA) => {
            return Err(format!(
                "unsupported schema {schema}; this tool reads schema {SCHEMA}"
            ));
        }
        None => match object.get("format") {
            Some(format) if format.as_u64() != Some(SCHEMA) => {
                return Err(format!(
                    "unsupported format {format}; this tool reads format {SCHEMA}"
                ));
            }
            None => {
                return Err(
                    "not a build_times.json: it needs \"schema\": 1 (or the Python record's \"format\": 1)"
                        .to_owned(),
                );
            }
            _ => {}
        },
        _ => {}
    }
    let builds = object
        .get("builds")
        .and_then(Value::as_array)
        .ok_or_else(|| "not a build_times.json: it has no builds array".to_owned())?;
    if builds.is_empty() {
        return Err("the record has no builds".to_owned());
    }
    let builds = builds
        .iter()
        .enumerate()
        .map(|(index, build)| parse_build(build, index))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Parsed::Times(TimesFile { builds }))
}

pub fn select_build<'a>(times: &'a TimesFile, select: Select) -> Result<&'a Build, String> {
    match select {
        Select::Latest => times
            .builds
            .last()
            .ok_or_else(|| "the record has no builds".to_owned()),
        Select::One(0) => Err("a build number starts at 1".to_owned()),
        Select::One(number) => times.builds.get(number - 1).ok_or_else(|| {
            format!(
                "build {number} is not in the record: it has {}",
                times.builds.len()
            )
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Regression,
    Missing,
    New,
}

impl Verdict {
    pub fn key(self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::Regression => "regressed",
            Verdict::Missing => "missing",
            Verdict::New => "new",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metric {
    pub baseline: Option<f64>,
    pub run: Option<f64>,
    pub limit: Option<f64>,
    pub delta_percent: Option<f64>,
    pub verdict: Verdict,
}

fn metric(baseline: Option<f64>, run: Option<f64>, tolerance: Tolerance) -> Metric {
    match (baseline, run) {
        (Some(baseline), Some(run)) => {
            let limit = tolerance.limit(baseline);
            Metric {
                baseline: Some(baseline),
                run: Some(run),
                limit: Some(limit),
                delta_percent: (baseline > 0.0).then(|| (run - baseline) / baseline * 100.0),
                verdict: if run > limit {
                    Verdict::Regression
                } else {
                    Verdict::Ok
                },
            }
        }
        (Some(baseline), None) => Metric {
            baseline: Some(baseline),
            run: None,
            limit: Some(tolerance.limit(baseline)),
            delta_percent: None,
            verdict: Verdict::Missing,
        },
        (None, Some(run)) => Metric {
            baseline: None,
            run: Some(run),
            limit: None,
            delta_percent: None,
            verdict: Verdict::New,
        },
        (None, None) => unreachable!("a metric needs one side"),
    }
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub tolerance: Tolerance,
    pub stages: Vec<(Stage, Metric)>,
    pub total: Option<Metric>,
    pub regressions: Vec<Stage>,
    pub missing: Vec<Stage>,
    pub total_regressed: bool,
    pub passed: bool,
}

/// Compare a build against a baseline's stages and total. A baseline stage
/// the run does not have is a failure unless `allow_missing`; a stage the
/// baseline does not have is reported as new and does not fail.
pub fn compare(
    build: &Build,
    baseline: &BTreeMap<Stage, f64>,
    baseline_total: Option<f64>,
    tolerance: Tolerance,
    allow_missing: bool,
) -> Comparison {
    let mut stages = Vec::new();
    for stage in Stage::CORE.into_iter().chain(Stage::EXTRA) {
        let before = baseline.get(&stage).copied();
        let now = build.stages.get(&stage).copied();
        if before.is_none() && now.is_none() {
            continue;
        }
        stages.push((stage, metric(before, now, tolerance)));
    }
    let total =
        baseline_total.map(|before| metric(Some(before), Some(build.total_seconds), tolerance));
    let regressions: Vec<Stage> = stages
        .iter()
        .filter(|(_, metric)| metric.verdict == Verdict::Regression)
        .map(|(stage, _)| *stage)
        .collect();
    let missing: Vec<Stage> = stages
        .iter()
        .filter(|(_, metric)| metric.verdict == Verdict::Missing)
        .map(|(stage, _)| *stage)
        .collect();
    let total_regressed = total
        .as_ref()
        .is_some_and(|metric| metric.verdict == Verdict::Regression);
    let passed =
        regressions.is_empty() && (allow_missing || missing.is_empty()) && !total_regressed;
    Comparison {
        tolerance,
        stages,
        total,
        regressions,
        missing,
        total_regressed,
        passed,
    }
}

fn round(seconds: f64, places: u32) -> f64 {
    let factor = 10f64.powi(places as i32);
    (seconds * factor).round() / factor
}

fn seconds_text(seconds: Option<f64>) -> String {
    match seconds {
        Some(seconds) => format!("{seconds:.1} s"),
        None => "-".to_owned(),
    }
}

fn percent_text(seconds: Option<f64>, total: f64) -> String {
    match seconds {
        Some(seconds) if total > 0.0 => format!("{:.1}%", seconds / total * 100.0),
        _ => "-".to_owned(),
    }
}

fn delta_text(delta: Option<f64>) -> String {
    match delta {
        Some(delta) => format!("{delta:+.1}%"),
        None => "-".to_owned(),
    }
}

fn table_rows(build: &Build) -> Vec<Stage> {
    let mut rows = Stage::CORE.to_vec();
    for stage in Stage::EXTRA {
        if build.stages.contains_key(&stage) {
            rows.push(stage);
        }
    }
    rows
}

/// The stage table the build itself ends with, from the canonical stages.
pub fn table(build: &Build) -> String {
    let mut out = String::new();
    out.push_str("where the time went:\n");
    let _ = writeln!(out, "{:<34} {:>10} {:>8}", "stage", "this run", "share");
    for stage in table_rows(build) {
        let seconds = build.stages.get(&stage).copied();
        let _ = writeln!(
            out,
            "{:<34} {:>10} {:>8}",
            stage.label(),
            seconds_text(seconds),
            percent_text(seconds, build.total_seconds)
        );
    }
    let _ = writeln!(
        out,
        "{:<34} {:>10} {:>8}",
        "total",
        seconds_text(Some(build.total_seconds)),
        "100.0%"
    );
    out
}

/// A comparison table: baseline, run, change, the tolerance's limit and the
/// verdict for each stage the two records have.
pub fn comparison_table(comparison: &Comparison) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<34} {:>10} {:>10} {:>9} {:>10}  status",
        "stage", "baseline", "this run", "change", "limit"
    );
    for (stage, metric) in &comparison.stages {
        let _ = writeln!(
            out,
            "{:<34} {:>10} {:>10} {:>9} {:>10}  {}",
            stage.label(),
            seconds_text(metric.baseline),
            seconds_text(metric.run),
            delta_text(metric.delta_percent),
            seconds_text(metric.limit),
            metric.verdict.key()
        );
    }
    if let Some(metric) = &comparison.total {
        let _ = writeln!(
            out,
            "{:<34} {:>10} {:>10} {:>9} {:>10}  {}",
            "total",
            seconds_text(metric.baseline),
            seconds_text(metric.run),
            delta_text(metric.delta_percent),
            seconds_text(metric.limit),
            metric.verdict.key()
        );
    }
    out
}

/// The one-line result a person (or a build log) reads.
pub fn comparison_result(comparison: &Comparison) -> String {
    let tolerance = format!(
        "{:.1}% + {:.1} s",
        comparison.tolerance.percent, comparison.tolerance.seconds
    );
    if comparison.passed {
        return format!("result: no regression beyond {tolerance}");
    }
    let mut parts = Vec::new();
    if !comparison.regressions.is_empty() {
        parts.push(format!(
            "{} regressed ({})",
            comparison.regressions.len(),
            comparison
                .regressions
                .iter()
                .map(|stage| stage.key())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if comparison.total_regressed {
        parts.push("the total regressed".to_owned());
    }
    if !comparison.missing.is_empty() {
        parts.push(format!(
            "{} missing ({})",
            comparison.missing.len(),
            comparison
                .missing
                .iter()
                .map(|stage| stage.key())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    format!("result: {} beyond {tolerance}", parts.join(", "))
}

fn metric_json(metric: &Metric) -> Value {
    json!({
        "baseline_seconds": metric.baseline.map(|seconds| round(seconds, 3)),
        "run_seconds": metric.run.map(|seconds| round(seconds, 3)),
        "limit_seconds": metric.limit.map(|seconds| round(seconds, 3)),
        "delta_percent": metric.delta_percent.map(|percent| round(percent, 1)),
        "verdict": metric.verdict.key(),
    })
}

fn build_json(build: &Build) -> Value {
    let stages = table_rows(build)
        .into_iter()
        .map(|stage| {
            let seconds = build.stages.get(&stage).copied();
            json!({
                "stage": stage.key(),
                "seconds": seconds.map(|seconds| round(seconds, 3)),
                "share_percent": seconds
                    .filter(|_| build.total_seconds > 0.0)
                    .map(|seconds| round(seconds / build.total_seconds * 100.0, 1)),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "total_seconds": round(build.total_seconds, 3),
        "compile_seconds": round(build.compile_seconds, 3),
        "stages": stages,
    })
}

/// The stable, game-path-free JSON a PR can carry. It has no build date, no
/// file name and no field the record did not need for its numbers.
pub fn summary_json(build: &Build, comparison: Option<&Comparison>) -> Value {
    let mut root = json!({
        "schema": SCHEMA,
        "tool": "build_times",
        "build": build_json(build),
    });
    if let Some(comparison) = comparison {
        root["comparison"] = json!({
            "tolerance": {
                "percent": round(comparison.tolerance.percent, 3),
                "seconds": round(comparison.tolerance.seconds, 3),
            },
            "passed": comparison.passed,
            "regressions": comparison.regressions.iter().map(|stage| stage.key()).collect::<Vec<_>>(),
            "missing": comparison.missing.iter().map(|stage| stage.key()).collect::<Vec<_>>(),
            "total_regressed": comparison.total_regressed,
            "stages": comparison.stages.iter().map(|(stage, metric)| {
                let mut value = metric_json(metric);
                value["stage"] = json!(stage.key());
                value
            }).collect::<Vec<_>>(),
            "total": comparison.total.as_ref().map(metric_json),
        });
    }
    root
}

/// The Markdown twin of `summary_json`, with the same stable order.
pub fn summary_markdown(build: &Build, comparison: Option<&Comparison>) -> String {
    let mut out = String::new();
    out.push_str("# Build timing\n\n");
    out.push_str("| Stage | This run | Share |\n|---|---:|---:|\n");
    for stage in table_rows(build) {
        let seconds = build.stages.get(&stage).copied();
        let _ = writeln!(
            out,
            "| {} | {} | {} |",
            stage.key(),
            seconds_text(seconds),
            percent_text(seconds, build.total_seconds)
        );
    }
    let _ = writeln!(
        out,
        "| **total** | **{}** | **100.0%** |",
        seconds_text(Some(build.total_seconds))
    );
    if let Some(comparison) = comparison {
        out.push_str("\n## Against baseline\n\n");
        let _ = writeln!(
            out,
            "Tolerance: +{:.1}% + {:.1} s per stage.",
            comparison.tolerance.percent, comparison.tolerance.seconds
        );
        out.push_str("\n| Stage | Baseline | This run | Change | Limit | Result |\n");
        out.push_str("|---|---:|---:|---:|---:|---|\n");
        for (stage, metric) in &comparison.stages {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} |",
                stage.key(),
                seconds_text(metric.baseline),
                seconds_text(metric.run),
                delta_text(metric.delta_percent),
                seconds_text(metric.limit),
                metric.verdict.key()
            );
        }
        if let Some(metric) = &comparison.total {
            let _ = writeln!(
                out,
                "| **total** | {} | {} | {} | {} | {} |",
                seconds_text(metric.baseline),
                seconds_text(metric.run),
                delta_text(metric.delta_percent),
                seconds_text(metric.limit),
                metric.verdict.key()
            );
        }
        let _ = writeln!(out, "\n{}\n", comparison_result(comparison));
    }
    out
}

/// Write the JSON and/or Markdown summary beside a build log.
pub fn write_summaries(
    build: &Build,
    comparison: Option<&Comparison>,
    json_path: Option<&Path>,
    markdown_path: Option<&Path>,
) -> Result<(), String> {
    if let Some(path) = json_path {
        let text = serde_json::to_string_pretty(&summary_json(build, comparison))
            .map_err(|error| format!("the JSON summary: {error}"))?;
        fs::write(path, format!("{text}\n"))
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if let Some(path) = markdown_path {
        fs::write(path, summary_markdown(build, comparison))
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

fn usage() -> &'static str {
    "build_times: show a build_times.json's stages and compare a run with a baseline\n\
     \n\
     usage:\n\
       build_times table <build_times.json> [--build latest|N]\n\
           [--json out.json] [--markdown out.md]\n\
       build_times compare <run.json> <baseline.json> [--build latest|N]\n\
           [--baseline-build latest|N] [--tolerance-percent PCT]\n\
           [--tolerance-seconds S] [--allow-missing] [--json out.json]\n\
           [--markdown out.md]\n\
     \n\
     A baseline is either a build_times.json (the latest build, or --baseline-build)\n\
     or an authored file: {\"schema\":1,\"baseline\":true,\"tolerance\":{\"percent\":10,\n\
     \"seconds\":2},\"total_seconds\":...,\"stages\":{...}}.\n\
     \n\
     exit: 0 inside tolerance, 1 a regression or missing stage, 2 unreadable input"
}

struct Args {
    command: Command,
    json: Option<PathBuf>,
    markdown: Option<PathBuf>,
    build: Select,
    baseline_build: Select,
    percent: Option<f64>,
    seconds: Option<f64>,
    allow_missing: bool,
}

enum Command {
    Table(PathBuf),
    Compare(PathBuf, PathBuf),
    Help,
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} needs a path"))
}

fn next_number(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<f64, String> {
    let text = args
        .next()
        .ok_or_else(|| format!("{flag} needs a number"))?;
    let number: f64 = text
        .parse()
        .map_err(|_| format!("{flag}: {text:?} is not a number"))?;
    if !number.is_finite() || number < 0.0 {
        return Err(format!("{flag}: {text:?} is not a non-negative number"));
    }
    Ok(number)
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let command = match args.next().as_deref() {
        None | Some("-h") | Some("--help") | Some("help") => Command::Help,
        Some("table") => {
            let file = args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "table needs a build_times.json".to_owned())?;
            Command::Table(file)
        }
        Some("compare") => {
            let run = args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "compare needs a run build_times.json".to_owned())?;
            let baseline = args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "compare needs a baseline".to_owned())?;
            Command::Compare(run, baseline)
        }
        Some(other) => return Err(format!("unknown command {other:?}; see --help")),
    };
    let mut parsed = Args {
        command,
        json: None,
        markdown: None,
        build: Select::Latest,
        baseline_build: Select::Latest,
        percent: None,
        seconds: None,
        allow_missing: false,
    };
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--json" => parsed.json = Some(next_path(&mut args, "--json")?),
            "--markdown" => parsed.markdown = Some(next_path(&mut args, "--markdown")?),
            "--build" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--build needs latest or a number".to_owned())?;
                parsed.build = Select::parse(&value)?;
            }
            "--baseline-build" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--baseline-build needs latest or a number".to_owned())?;
                parsed.baseline_build = Select::parse(&value)?;
            }
            "--tolerance-percent" => {
                parsed.percent = Some(next_number(&mut args, "--tolerance-percent")?)
            }
            "--tolerance-seconds" => {
                parsed.seconds = Some(next_number(&mut args, "--tolerance-seconds")?)
            }
            "--allow-missing" => parsed.allow_missing = true,
            "-h" | "--help" => parsed.command = Command::Help,
            other => return Err(format!("unknown option {other:?}; see --help")),
        }
    }
    Ok(parsed)
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn times(path: &Path) -> Result<TimesFile, String> {
    match parse(&read(path)?)? {
        Parsed::Times(times) => Ok(times),
        Parsed::Baseline(_) => Err(format!(
            "{} is an authored baseline, not a build_times.json",
            path.display()
        )),
    }
}

/// Run one command; the number is the process exit code.
pub fn run(args: Vec<String>) -> Result<i32, String> {
    let args = parse_args(args.into_iter())?;
    match args.command {
        Command::Help => {
            println!("{}", usage());
            Ok(0)
        }
        Command::Table(path) => {
            let times = times(&path)?;
            let build = select_build(&times, args.build)?;
            print!("{}", table(build));
            write_summaries(build, None, args.json.as_deref(), args.markdown.as_deref())?;
            Ok(0)
        }
        Command::Compare(run_path, baseline_path) => {
            let run_times = times(&run_path)?;
            let build = select_build(&run_times, args.build)?;
            let (baseline, baseline_total, embedded) = match parse(&read(&baseline_path)?)? {
                Parsed::Baseline(baseline) => {
                    (baseline.stages, baseline.total_seconds, baseline.tolerance)
                }
                Parsed::Times(times) => {
                    let build = select_build(&times, args.baseline_build)?;
                    (build.stages.clone(), Some(build.total_seconds), None)
                }
            };
            let tolerance = Tolerance {
                percent: args
                    .percent
                    .or(embedded.map(|tolerance| tolerance.percent))
                    .unwrap_or(DEFAULT_PERCENT),
                seconds: args
                    .seconds
                    .or(embedded.map(|tolerance| tolerance.seconds))
                    .unwrap_or(DEFAULT_SECONDS),
            };
            let comparison = compare(
                build,
                &baseline,
                baseline_total,
                tolerance,
                args.allow_missing,
            );
            print!("{}", comparison_table(&comparison));
            println!("{}", comparison_result(&comparison));
            write_summaries(
                build,
                Some(&comparison),
                args.json.as_deref(),
                args.markdown.as_deref(),
            )?;
            Ok(if comparison.passed { 0 } else { 1 })
        }
    }
}

#[cfg(not(test))]
fn main() {
    let code = match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("build_times: {message}");
            2
        }
    };
    std::process::exit(code);
}
