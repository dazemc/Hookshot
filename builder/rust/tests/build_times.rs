// Game-free checks of the build-time report and regression gate: the stage
// table, both the Rust record's stages and the Python record's phases, a
// missing stage, a stage over its baseline tolerance, and the stable summary.
// The fixtures under tests/fixtures/build_times/ are authored numbers; no
// game, dump or build is involved.

#[allow(dead_code)]
#[path = "../../../scripts/build_times.rs"]
mod tool;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/build_times")
        .join(name)
}

fn fixture_text(name: &str) -> String {
    let path = fixture(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn times(name: &str) -> tool::TimesFile {
    match tool::parse(&fixture_text(name)).unwrap() {
        tool::Parsed::Times(times) => times,
        tool::Parsed::Baseline(_) => panic!("{name} is a baseline, not a build record"),
    }
}

fn baseline(name: &str) -> tool::BaselineFile {
    match tool::parse(&fixture_text(name)).unwrap() {
        tool::Parsed::Baseline(baseline) => baseline,
        tool::Parsed::Times(_) => panic!("{name} is a build record, not a baseline"),
    }
}

fn run_build() -> tool::Build {
    tool::select_build(&times("run.json"), tool::Select::Latest)
        .unwrap()
        .clone()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-build-times-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn row<'a>(table: &'a str, name: &str) -> &'a str {
    table
        .lines()
        .find(|line| line.trim_start().starts_with(name))
        .unwrap_or_else(|| panic!("no {name} row in:\n{table}"))
}

#[test]
fn the_stage_table_lists_checks_read_lift_compile_links_and_finish() {
    let build = run_build();
    let table = tool::table(&build);
    let names = [
        "checks",
        "read",
        "lift",
        "compile",
        "planning",
        "compiling",
        "links",
        "finish",
    ];
    let mut last = 0;
    for name in names {
        let at = table
            .find(&format!("{name} "))
            .unwrap_or_else(|| panic!("no {name} in:\n{table}"));
        assert!(at >= last, "{name} is out of order in:\n{table}");
        last = at;
    }
    assert!(row(&table, "checks").contains('-'), "{table}");
    assert!(row(&table, "read").contains("50.5 s"), "{table}");
    assert!(row(&table, "lift").contains("25.8 s"), "{table}");
    assert!(row(&table, "compile").contains("828.0 s"), "{table}");
    assert!(row(&table, "planning").contains("9.0 s"), "{table}");
    assert!(row(&table, "compiling").contains("815.5 s"), "{table}");
    assert!(row(&table, "links").contains("3.5 s"), "{table}");
    assert!(row(&table, "finish").contains("10.2 s"), "{table}");
    assert!(row(&table, "total").contains("1200.0 s"), "{table}");
    assert!(table.starts_with("where the time went:"), "{table}");
}

#[test]
fn a_rust_record_maps_its_stage_labels_and_keeps_totals_from_double_counting_parts() {
    let build = run_build();
    let stage = |name| build.stages.get(&name).copied();
    assert_eq!(stage(tool::Stage::Read), Some(50.5));
    // (lift.total wins; lift.discover and lift.emit are parts, not more lift)
    assert_eq!(stage(tool::Stage::Lift), Some(25.8));
    assert_eq!(stage(tool::Stage::Compile), Some(828.0));
    assert_eq!(stage(tool::Stage::Planning), Some(9.0));
    assert_eq!(stage(tool::Stage::Compiling), Some(815.5));
    // (driver.link + driver.copy)
    assert_eq!(stage(tool::Stage::Links), Some(3.5));
    assert_eq!(stage(tool::Stage::Finish), Some(10.2));
    assert_eq!(stage(tool::Stage::Host), Some(95.0));
    assert_eq!(stage(tool::Stage::Vulkan), Some(100.0));
    assert_eq!(stage(tool::Stage::Assets), Some(2.5));
    assert_eq!(stage(tool::Stage::Shaders), Some(12.0));
    assert_eq!(stage(tool::Stage::Prepare), Some(20.0));
    assert_eq!(build.total_seconds, 1200.0);
    assert_eq!(build.compile_seconds, 828.0);
}

#[test]
fn a_python_phases_record_reads_as_the_same_stages() {
    let times = times("legacy-phases.json");
    let build = tool::select_build(&times, tool::Select::Latest).unwrap();
    let stage = |name| build.stages.get(&name).copied();
    assert_eq!(stage(tool::Stage::Checks), Some(2.0));
    assert_eq!(stage(tool::Stage::Read), Some(10.0));
    assert_eq!(stage(tool::Stage::Lift), Some(331.0));
    assert_eq!(stage(tool::Stage::Compile), Some(1300.0));
    assert_eq!(stage(tool::Stage::Planning), Some(9.0));
    assert_eq!(stage(tool::Stage::Compiling), Some(1282.0));
    // (both "compile: linking ..." lines add up)
    assert_eq!(stage(tool::Stage::Links), Some(3.0));
    assert_eq!(stage(tool::Stage::Finish), Some(57.0));
    let table = tool::table(build);
    assert!(row(&table, "checks").contains("2.0 s"), "{table}");
    assert!(row(&table, "links").contains("3.0 s"), "{table}");
}

#[test]
fn a_record_without_a_known_schema_or_with_a_bad_stage_is_refused() {
    for text in [
        r#"{"builds":[]}"#,
        r#"{"schema":2,"builds":[]}"#,
        r#"{"format":2,"builds":[]}"#,
        r#"{"schema":1}"#,
        r#"{"schema":1,"builds":[]}"#,
        r#"{"schema":1,"builds":[{"total_seconds":1,"stages":{"read":"soon"}}]}"#,
        r#"{"schema":1,"builds":[{"total_seconds":1,"stages":{"read":-2}}]}"#,
        r#"{"schema":1,"builds":[{"stages":{"read":1}}]}"#,
    ] {
        assert!(tool::parse(text).is_err(), "accepted {text}");
    }
    assert!(tool::parse(r#"{"schema":1,"baseline":true,"stages":{}}"#).is_err());
    assert!(
        tool::parse(
            r#"{"schema":1,"baseline":true,"tolerance":{"percent":-1},"stages":{"read":1}}"#
        )
        .is_err()
    );
}

#[test]
fn a_missing_stage_is_named_and_fails_the_comparison() {
    let baseline = baseline("baseline.json");
    let mut build = run_build();
    build.stages.remove(&tool::Stage::Read);
    let comparison = tool::compare(
        &build,
        &baseline.stages,
        baseline.total_seconds,
        baseline.tolerance.unwrap(),
        false,
    );
    assert!(!comparison.passed);
    assert_eq!(comparison.missing, vec![tool::Stage::Read]);
    let table = tool::comparison_table(&comparison);
    assert!(row(&table, "read").contains("missing"), "{table}");
    let result = tool::comparison_result(&comparison);
    assert!(result.contains("1 missing (read)"), "{result}");
    let json = tool::summary_json(&build, Some(&comparison));
    assert_eq!(json["comparison"]["missing"][0], "read");
    assert_eq!(json["comparison"]["passed"], false);
    assert!(tool::summary_markdown(&build, Some(&comparison)).contains("| read | 50.0 s | - |"));

    // (a record from a tool version that stopped recording a stage can still
    // be reported: --allow-missing says so without hiding a real regression)
    let allowed = tool::compare(
        &build,
        &baseline.stages,
        baseline.total_seconds,
        baseline.tolerance.unwrap(),
        true,
    );
    assert!(allowed.passed);
    assert_eq!(allowed.missing, vec![tool::Stage::Read]);
}

#[test]
fn a_stage_over_tolerance_regresses_and_one_inside_it_passes() {
    let baseline = baseline("baseline.json");
    let tolerance = baseline.tolerance.unwrap();

    let mut inside = run_build();
    inside.stages.insert(tool::Stage::Read, 50.0 * 1.05);
    let comparison = tool::compare(
        &inside,
        &baseline.stages,
        baseline.total_seconds,
        tolerance,
        false,
    );
    assert!(
        comparison.passed,
        "{}",
        tool::comparison_result(&comparison)
    );
    assert!(row(&tool::comparison_table(&comparison), "read").contains("ok"));

    let mut over = run_build();
    over.stages.insert(tool::Stage::Lift, 45.0);
    let comparison = tool::compare(
        &over,
        &baseline.stages,
        baseline.total_seconds,
        tolerance,
        false,
    );
    assert!(!comparison.passed);
    assert_eq!(comparison.regressions, vec![tool::Stage::Lift]);
    let table = tool::comparison_table(&comparison);
    assert!(row(&table, "lift").contains("regressed"), "{table}");
    assert!(row(&table, "lift").contains("45.0 s"), "{table}");
    assert!(tool::comparison_result(&comparison).contains("1 regressed (lift)"));
    let json = tool::summary_json(&over, Some(&comparison));
    assert_eq!(json["comparison"]["regressions"][0], "lift");
    assert_eq!(json["comparison"]["passed"], false);
    assert!(tool::summary_markdown(&over, Some(&comparison)).contains("regressed"));

    // (a total over its own tolerance fails too, even when every stage fits)
    let mut total = run_build();
    total.total_seconds = 1400.0;
    let comparison = tool::compare(
        &total,
        &baseline.stages,
        baseline.total_seconds,
        tolerance,
        false,
    );
    assert!(!comparison.passed);
    assert!(comparison.total_regressed);
    assert!(tool::comparison_result(&comparison).contains("the total regressed"));
}

#[test]
fn a_times_file_can_be_its_own_baseline() {
    let build = run_build();
    let comparison = tool::compare(
        &build,
        &build.stages,
        Some(build.total_seconds),
        tool::Tolerance::default(),
        false,
    );
    assert!(comparison.passed);
    assert!(comparison.regressions.is_empty());
    assert!(comparison.missing.is_empty());
}

#[test]
fn the_summaries_are_stable_and_carry_no_path_or_date() {
    let marker = "/home/someone/Games/zelda/romfs/exefs";
    let text = fixture_text("run.json")
        .replace(
            "\"when\": \"2026-11-12 12:00:00\"",
            &format!("\"when\": \"{marker}\""),
        )
        .replace("\"kind\": \"build\"", &format!("\"kind\": \"{marker}\""));
    let times = match tool::parse(&text).unwrap() {
        tool::Parsed::Times(times) => times,
        tool::Parsed::Baseline(_) => panic!("the run is a baseline"),
    };
    let build = tool::select_build(&times, tool::Select::Latest).unwrap();
    let baseline = baseline("baseline.json");
    let comparison = tool::compare(
        build,
        &baseline.stages,
        baseline.total_seconds,
        baseline.tolerance.unwrap(),
        false,
    );
    let json = serde_json::to_string_pretty(&tool::summary_json(build, Some(&comparison))).unwrap();
    let markdown = tool::summary_markdown(build, Some(&comparison));
    assert_eq!(
        json,
        serde_json::to_string_pretty(&tool::summary_json(build, Some(&comparison))).unwrap()
    );
    assert_eq!(markdown, tool::summary_markdown(build, Some(&comparison)));
    for output in [&json, &markdown] {
        assert!(!output.contains(marker), "{output}");
        assert!(!output.contains("/home/"), "{output}");
        assert!(!output.contains("romfs"), "{output}");
        assert!(!output.contains("exefs"), "{output}");
        assert!(!output.contains("2026-11-12"), "{output}");
        assert!(!output.contains("\"when\""), "{output}");
    }
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["tool"], "build_times");
    assert_eq!(value["build"]["total_seconds"], 1200.0);
    assert_eq!(value["comparison"]["passed"], true);
}

#[test]
fn the_cli_prints_the_table_and_writes_the_two_summaries() {
    let dir = scratch("cli");
    let json = dir.join("build-times.json");
    let markdown = dir.join("build-times.md");
    let output = Command::new(env!("CARGO_BIN_EXE_build_times"))
        .arg("table")
        .arg(fixture("run.json"))
        .arg("--json")
        .arg(&json)
        .arg("--markdown")
        .arg(&markdown)
        .output()
        .expect("run build_times table");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("where the time went:"), "{stdout}");
    assert!(stdout.contains("compiling"), "{stdout}");
    let text = fs::read_to_string(&json).unwrap();
    assert!(!text.contains(dir.to_str().unwrap()), "{text}");
    assert!(fs::read_to_string(&markdown).unwrap().contains("| Stage |"));

    let output = Command::new(env!("CARGO_BIN_EXE_build_times"))
        .arg("compare")
        .arg(fixture("run.json"))
        .arg(fixture("baseline.json"))
        .arg("--json")
        .arg(&json)
        .arg("--markdown")
        .arg(&markdown)
        .output()
        .expect("run build_times compare");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("no regression"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(value["comparison"]["passed"], true);

    // (the authored baseline's 25 s lift does not allow a 45 s one)
    let regressed = dir.join("regressed.json");
    fs::write(
        &regressed,
        fixture_text("run.json").replace("\"lift.total\": 25.8", "\"lift.total\": 45.0"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_build_times"))
        .arg("compare")
        .arg(&regressed)
        .arg(fixture("baseline.json"))
        .output()
        .expect("run build_times compare");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("regressed"), "{stdout}");
    assert!(stdout.contains("lift"), "{stdout}");

    let bad = dir.join("bad.json");
    fs::write(&bad, "{\"builds\": []}").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_build_times"))
        .arg("table")
        .arg(&bad)
        .output()
        .expect("run build_times table");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("schema"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(dir);
}
