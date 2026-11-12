//! The host CPU level and the builder's worker count. Ported from
//! nx2/host_cpu.py and make_game.py's job selection.

use serde_json::{Value, json};

/// The x86-64 level this PC runs: x86-64-v3 has AVX2, FMA, BMI2 and MOVBE.
pub fn march() -> String {
    #[cfg(windows)]
    {
        if windows::feature(40) {
            return "x86-64-v3".into();
        }
        return if windows::feature(38) {
            "x86-64-v2"
        } else {
            "x86-64"
        }
        .into();
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") {
            if let Some(line) = text.lines().find(|line| line.starts_with("flags")) {
                let flags: Vec<&str> = line
                    .split(':')
                    .nth(1)
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                let has = |name: &str| flags.contains(&name);
                if ["avx2", "fma", "bmi2", "movbe"]
                    .iter()
                    .all(|name| has(name))
                {
                    return "x86-64-v3".into();
                }
                if ["sse4_2", "popcnt", "ssse3"].iter().all(|name| has(name)) {
                    return "x86-64-v2".into();
                }
            }
        }
        "x86-64".into()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        "x86-64-v3".into()
    }
}

#[cfg(windows)]
mod windows {
    unsafe extern "system" {
        fn IsProcessorFeaturePresent(feature: u32) -> i32;
    }
    pub fn feature(number: u32) -> bool {
        unsafe { IsProcessorFeaturePresent(number) != 0 }
    }
}

/// The tuning target for the host CPU: a Steam Deck's APU (0405 on the LCD,
/// 0932 on the OLED) is Zen 2, and clang does not schedule a generic -march
/// level for it. None elsewhere: an unknown CPU is left to its -march level.
pub fn mtune() -> Option<String> {
    let deck = std::env::var("RX_STEAMDECK").ok().as_deref() == Some("1");
    #[cfg(target_os = "linux")]
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok();
    #[cfg(not(target_os = "linux"))]
    let cpuinfo: Option<String> = None;
    mtune_with(deck, cpuinfo.as_deref())
}

fn mtune_with(deck: bool, cpuinfo: Option<&str>) -> Option<String> {
    if deck {
        return Some("znver2".into());
    }
    cpuinfo.and_then(mtune_text).map(str::to_owned)
}

/// The APU names a Steam Deck reports in /proc/cpuinfo's model name.
fn mtune_text(cpuinfo: &str) -> Option<&'static str> {
    if cpuinfo.contains("Custom APU 0405") || cpuinfo.contains("Custom APU 0932") {
        Some("znver2")
    } else {
        None
    }
}

/// RX_MTUNE when the player named one, else what this PC's CPU asks for.
pub fn mtune_flag() -> Option<String> {
    mtune_flag_with(std::env::var("RX_MTUNE").ok().as_deref(), mtune())
}

fn mtune_flag_with(named: Option<&str>, detected: Option<String>) -> Option<String> {
    named
        .filter(|tune| !tune.is_empty())
        .map(str::to_owned)
        .or(detected)
}

/// The CPU flags a compiler gets: the level this PC runs, and the tune its
/// cores ask for when one is known.
pub fn cpu_flags() -> Vec<String> {
    cpu_flags_with(&march(), mtune_flag().as_deref())
}

fn cpu_flags_with(level: &str, tune: Option<&str>) -> Vec<String> {
    let mut flags = vec![format!("-march={level}")];
    if let Some(tune) = tune {
        flags.push(format!("-mtune={tune}"));
    }
    flags
}

pub fn cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
}

/// HOOKSHOT_JOBS or --jobs: 0 means automatic, negative and junk are errors.
pub fn requested_jobs(value: Option<&str>) -> Result<Option<usize>, String> {
    let error = "HOOKSHOT_JOBS must be a whole number: use 1 or more, or 0 for automatic.";
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let jobs: i64 = value.parse().map_err(|_| error.to_owned())?;
    if jobs < 0 {
        return Err(error.into());
    }
    Ok(Some(jobs as usize).filter(|jobs| *jobs > 0))
}

pub fn chosen_jobs(given: Option<usize>, environment: Option<&str>) -> Result<usize, String> {
    if let Some(given) = given.filter(|given| *given > 0) {
        return Ok(given);
    }
    if let Some(jobs) = requested_jobs(environment)? {
        return Ok(jobs);
    }
    Ok(cpus())
}

pub fn json() -> Value {
    json!({
        "march": march(),
        "mtune": mtune(),
        "cpus": cpus(),
        "prebuilt_vulkan": crate::kit::env_on("HOOKSHOT_VK_PREBUILT"),
        "march_override": std::env::var("RX_MARCH").ok(),
        "mtune_override": std::env::var("RX_MTUNE").ok(),
    })
}

/// Issue 363: the AppImage's renderer needs AVX2; say so before reading a
/// 25 GB package, not 40 minutes into a build.
pub fn check_json() -> Result<Value, String> {
    let level = march();
    let prebuilt = crate::kit::env_on("HOOKSHOT_VK_PREBUILT") && std::env::var("RX_MARCH").is_err();
    let problem = (prebuilt && level != "x86-64-v3").then(|| {
        format!(
            "Your processor has no AVX2 (it runs {level}), and the AppImage's renderer needs it. Build Hookshot from source instead (see the readme): a source build follows your processor."
        )
    });
    Ok(json!({
        "march": level,
        "cpus": cpus(),
        "prebuilt_vulkan": prebuilt,
        "problem": problem,
        "ok": problem.is_none(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_follow_the_switches() {
        assert_eq!(chosen_jobs(Some(3), None), Ok(3));
        assert_eq!(chosen_jobs(None, Some("2")), Ok(2));
        assert_eq!(chosen_jobs(None, Some("0")), Ok(cpus()));
        assert!(chosen_jobs(None, Some("-1")).is_err());
        assert!(chosen_jobs(None, Some("many")).is_err());
        assert_eq!(requested_jobs(Some("")), Ok(None));
    }

    #[test]
    fn a_march_is_named() {
        let level = march();
        assert!(matches!(
            level.as_str(),
            "x86-64" | "x86-64-v2" | "x86-64-v3"
        ));
    }

    #[test]
    fn a_steam_deck_apu_is_tuned_for_zen_two() {
        assert_eq!(mtune_with(true, None), Some("znver2".into()));
        assert_eq!(
            mtune_with(false, Some("model name\t: AMD Custom APU 0405\n")),
            Some("znver2".into())
        );
        assert_eq!(
            mtune_with(false, Some("model name\t: AMD Custom APU 0932\n")),
            Some("znver2".into())
        );
        assert_eq!(
            mtune_with(false, Some("model name\t: AMD Ryzen 9 7900X\n")),
            None
        );
        assert_eq!(mtune_with(false, None), None);
    }

    #[test]
    fn a_tune_is_zen_two_or_absent() {
        let tune = mtune();
        assert!(
            tune.is_none() || tune.as_deref() == Some("znver2"),
            "{tune:?}"
        );
    }

    #[test]
    fn rx_mtune_is_the_players_choice() {
        assert_eq!(
            mtune_flag_with(Some("skylake"), Some("znver2".into())).as_deref(),
            Some("skylake")
        );
        assert_eq!(
            mtune_flag_with(Some(""), Some("znver2".into())).as_deref(),
            Some("znver2")
        );
        assert_eq!(mtune_flag_with(None, None), None);
    }

    #[test]
    fn the_cpu_flags_name_the_level_and_any_tune() {
        assert_eq!(cpu_flags_with("x86-64-v3", None), vec!["-march=x86-64-v3"]);
        assert_eq!(
            cpu_flags_with("x86-64-v3", Some("znver2")),
            vec!["-march=x86-64-v3", "-mtune=znver2"]
        );
        let flags = cpu_flags();
        assert!(flags[0].starts_with("-march="), "{flags:?}");
    }
}
