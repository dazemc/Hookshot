//! The standalone build's lift-time pieces, ported from nx2/standalone.py:
//! the SDK functions the native runtime implements, the stubs for the
//! OS-facing imports it does not, the NVN export table, and the boot
//! constants hos_main.c reads.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use crate::image::Image;
use crate::nso_image::Nso;

/// The SDK namespaces whose free functions talk to system services.
const OS_NAMESPACES: [&str; 42] = [
    "audio", "hid", "oe", "prepo", "nfp", "time", "account", "vi", "swkbd", "pl", "err", "codec",
    "diag", "detail", "settings", "ns", "applet", "album", "friends", "nifm", "ssl", "socket",
    "ec", "bcat", "web", "ldn", "irsensor", "aoc", "olsc", "pctl", "apm", "ro", "chat", "camera",
    "gs", "movie", "npns", "news", "fatal", "lbl", "capture", "syncpt",
];

fn implemented(host: &Path) -> Result<HashSet<String>, String> {
    let imp = regex::Regex::new(r"(?m)^void (imp_\w+)\(Ctx").unwrap();
    let hle = regex::Regex::new(r"(?m)^HLE\((\w+)\)").unwrap();
    let mut names = HashSet::new();
    for entry in fs::read_dir(host).map_err(|e| format!("{}: {e}", host.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("c") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_default();
        for caps in imp.captures_iter(&text) {
            names.insert(caps[1].to_owned());
        }
        for caps in hle.captures_iter(&text) {
            names.insert(format!("imp_{}", &caps[1]));
        }
    }
    Ok(names)
}

fn nested_name(symbol: &str) -> Option<Vec<&str>> {
    let mut i = if symbol.starts_with("_ZNK") {
        4
    } else if symbol.starts_with("_ZN") {
        3
    } else {
        return None;
    };
    let bytes = symbol.as_bytes();
    let mut parts = Vec::new();
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let len: usize = symbol[start..i].parse().ok()?;
        let end = i.checked_add(len)?;
        parts.push(symbol.get(i..end)?);
        i = end;
    }
    (symbol.as_bytes().get(i) == Some(&b'E')).then_some(parts)
}

fn os_facing(symbol: &str) -> bool {
    let Some(parts) = nested_name(symbol) else {
        return false;
    };
    parts.first() == Some(&"nn")
        && parts
            .get(1)
            .is_some_and(|name| OS_NAMESPACES.contains(name))
        && (parts.len() == 3 || (parts.len() == 4 && parts.get(2) == Some(&"sdk")))
}

fn nvn_proc_name(symbol: &str) -> Option<String> {
    if symbol.starts_with("nvn") {
        return Some(symbol.to_owned());
    }
    let rest = symbol.strip_prefix("_Z")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let len: usize = rest.get(..digits)?.parse().ok()?;
    let name = rest.get(digits..digits + len)?;
    name.starts_with("nvn").then(|| name.to_owned())
}

fn nvn_exports(modules: &[Nso], have: &HashSet<String>) -> Vec<String> {
    let mut names = Vec::new();
    for module in modules.iter().filter(|module| module.name == "sdk") {
        for symbol in &module.symbols {
            if !symbol.defined || symbol.name.is_empty() || symbol.kind != "func" {
                continue;
            }
            let Some(name) = nvn_proc_name(&symbol.name) else {
                continue;
            };
            if !have.contains(&format!("imp_{name}")) {
                names.push(symbol.name.clone());
            }
        }
    }
    names.sort();
    names
}

fn hle_set(modules: &[Nso], implemented: &HashSet<String>) -> (HashSet<String>, HashSet<String>) {
    let hle: HashSet<String> = implemented
        .iter()
        .map(|name| name.strip_prefix("imp_").unwrap_or(name).to_owned())
        .collect();
    let known: HashSet<&str> = modules
        .iter()
        .flat_map(|module| module.symbols.iter())
        .map(|symbol| symbol.name.as_str())
        .filter(|name| !name.is_empty())
        .collect();
    let hle: HashSet<String> = hle
        .into_iter()
        .filter(|name| known.contains(name.as_str()))
        .collect();
    let kind: HashMap<&str, &str> = modules
        .iter()
        .flat_map(|module| module.symbols.iter())
        .filter(|symbol| symbol.defined && !symbol.name.is_empty())
        .map(|symbol| (symbol.name.as_str(), symbol.kind.as_str()))
        .collect();
    let mut stubbed = HashSet::new();
    for module in modules
        .iter()
        .filter(|module| module.name != "sdk" && module.name != "rtld")
    {
        for symbol in &module.symbols {
            if !symbol.defined
                && !symbol.name.is_empty()
                && os_facing(&symbol.name)
                && kind.get(symbol.name.as_str()).copied().unwrap_or("func") == "func"
            {
                stubbed.insert(symbol.name.clone());
            }
        }
    }
    stubbed.retain(|name| !hle.contains(name));
    (hle, stubbed)
}

fn demangle(symbols: &[String]) -> HashMap<String, String> {
    let mut child = match Command::new(crate::toolchain::find("llvm-cxxfilt", false))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return symbols.iter().map(|s| (s.clone(), s.clone())).collect(),
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = writeln!(stdin, "{}", symbols.join("\n"));
    }
    let output = child.wait_with_output().ok();
    let lines: Vec<String> = output
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if lines.len() == symbols.len() {
        symbols.iter().cloned().zip(lines).collect()
    } else {
        symbols.iter().map(|s| (s.clone(), s.clone())).collect()
    }
}

fn gen_stubs(out_dir: &Path, meta: &Value, implemented: &HashSet<String>) -> Result<usize, String> {
    let mut have = implemented.clone();
    for hook in meta["hooks"].as_array().into_iter().flatten() {
        if let Some(symbol) = hook["sym"].as_str()
            && nvn_proc_name(symbol).is_some()
        {
            have.insert(format!("imp_{symbol}"));
        }
    }
    let hooks: Vec<(&str, &str)> = meta["hooks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|hook| Some((hook["c"].as_str()?, hook["sym"].as_str()?)))
        .filter(|(name, _)| !have.contains(*name))
        .collect();
    let symbols: Vec<String> = hooks.iter().map(|(_, sym)| (*sym).to_owned()).collect();
    let names = demangle(&symbols);
    let mut lines = String::from(
        "#include \"a64.h\"\nvoid a64_unimplemented(Ctx *A64_NOESC c, const char *sym, int *seen);\n\n",
    );
    for (name, symbol) in &hooks {
        lines.push_str(&format!(
            "/* {} */\nvoid {}(Ctx *A64_NOESC c) {{ static int seen; a64_unimplemented(c, \"{}\", &seen); }}\n",
            names
                .get(*symbol)
                .map(String::as_str)
                .unwrap_or(symbol)
                .replace("*/", "* /"),
            name,
            symbol
        ));
    }
    fs::write(out_dir.join("a64_stubs.c"), lines).map_err(|e| e.to_string())?;
    Ok(hooks.len())
}

fn write_nvn_exports(out_dir: &Path, names: &[String]) -> Result<(), String> {
    let mut text = String::from(
        "/* Generated by nx2/lift.py --standalone: the sdk's NVN functions, routed to host/nvn2.c. */\n#include \"a64.h\"\nvoid nvn2_dispatch(Ctx *c, const char *name, void **cache);\n\n",
    );
    for name in names {
        text.push_str(&format!(
            "void imp_{name}(Ctx *A64_NOESC c) {{ static void *k[2]; nvn2_dispatch(c, \"{}\", k); }}\n",
            nvn_proc_name(name).unwrap_or_else(|| name.clone())
        ));
    }
    fs::write(out_dir.join("nvn2_exports.c"), text).map_err(|e| e.to_string())?;
    Ok(())
}

fn thread_hooks(image: &Image, meta: &Value) -> [u64; 2] {
    for module in meta["modules"].as_array().into_iter().flatten() {
        let base = module["base"].as_u64().unwrap_or(0);
        let size = module["size"].as_u64().unwrap_or(0);
        let mut pc = base;
        while pc + 12 <= base + size {
            if image.in_code(pc)
                && image.word(pc) == Some(0xD53BD061)
                && image.word(pc + 4) == Some(0x9107E021)
                && image.word(pc + 8) == Some(0xF9000020)
            {
                let mut hooks = Vec::new();
                let mut page: HashMap<u32, u64> = HashMap::new();
                for q in (pc + 12..pc + 12 + 4 * 40).step_by(4) {
                    let Some(word) = image.word(q) else { break };
                    if word & 0x9F000000 == 0x90000000 {
                        let imm = (((word >> 3) & 0x1FFFFC) | ((word >> 29) & 3)) as i32;
                        let imm = if imm & (1 << 20) != 0 {
                            imm - (1 << 21)
                        } else {
                            imm
                        };
                        page.insert(word & 31, (q & !0xFFF) + ((imm as i64) << 12) as u64);
                    } else if word & 0xFFC00000 == 0xF9400000
                        && let Some(address) = page.remove(&((word >> 5) & 31))
                    {
                        let got = address + (((word >> 10) & 0xFFF) as u64) * 8;
                        if let Some(bytes) = image.read(got, 8) {
                            hooks.push(u64::from_le_bytes(bytes.try_into().unwrap()));
                        }
                        if hooks.len() == 2 {
                            return [hooks[0], hooks[1]];
                        }
                    }
                }
            }
            pc += 4;
        }
    }
    [0, 0]
}

fn write_image_c(
    out_dir: &Path,
    image: &Image,
    meta: &Value,
    exefs: &Path,
) -> Result<[u64; 2], String> {
    let raw = fs::read(exefs.join("main.npdm")).map_err(|e| e.to_string())?;
    if raw.get(..4) != Some(b"META") {
        return Err("main.npdm: no META header".into());
    }
    if raw.len() < 0x78 {
        return Err("main.npdm is too small".into());
    }
    let prio = raw[0xE];
    let core = raw[0xF];
    let stack = u32::from_le_bytes(raw[0x1C..0x20].try_into().unwrap()) as u64;
    let aoff = u32::from_le_bytes(raw[0x70..0x74].try_into().unwrap()) as usize;
    let asz = u32::from_le_bytes(raw[0x74..0x78].try_into().unwrap()) as usize;
    let aci = raw.get(aoff..aoff + asz).unwrap_or_default();
    let program_id = if aci.get(..4) == Some(b"ACI0") && aci.len() >= 0x18 {
        u64::from_le_bytes(aci[0x10..0x18].try_into().unwrap())
    } else {
        0
    };
    let modules: Vec<&Value> = meta["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|module| {
            module["init"].as_u64().unwrap_or(0) != 0 && module["name"].as_str() != Some("rtld")
        })
        .collect();
    let exports = |name: &str| meta["exports"][name].as_u64().unwrap_or(0);
    let mut lines =
        String::from("/* Generated by nx2/lift.py --standalone. */\n#include <stdint.h>\n");
    lines.push_str(&format!(
        "const uint64_t hos_main_stack_size = {stack:#x}ULL;\n"
    ));
    lines.push_str(&format!(
        "const int hos_main_priority = {prio}, hos_main_core = {core};\n"
    ));
    lines.push_str(&format!(
        "const uint64_t hos_image_base = {:#x}ULL, hos_image_end = {:#x}ULL;\n",
        meta["base"].as_u64().unwrap_or(0),
        meta["end"].as_u64().unwrap_or(0)
    ));
    lines.push_str(&format!(
        "const int hos_module_count = {};\n",
        modules.len()
    ));
    let inits: Vec<String> = modules
        .iter()
        .map(|module| format!("{:#x}ULL", module["init"].as_u64().unwrap_or(0)))
        .collect();
    lines.push_str(&format!(
        "const uint64_t hos_module_init[] = {{{}}};\n",
        inits.join(", ")
    ));
    let names: Vec<String> = modules
        .iter()
        .map(|module| format!("\"{}\"", module["name"].as_str().unwrap_or("")))
        .collect();
    lines.push_str(&format!(
        "const char *const hos_module_name[] = {{{}}};\n",
        names.join(", ")
    ));
    lines.push_str(&format!(
        "const uint64_t hos_entry_nninitStartup = {:#x}ULL;\n",
        exports("nninitStartup")
    ));
    lines.push_str(&format!(
        "const uint64_t hos_entry_nnMain = {:#x}ULL;\n",
        exports("nnMain")
    ));
    lines.push_str(&format!(
        "const uint64_t hos_program_id = {program_id:#x}ULL;   /* (main.npdm's ACI0) */\n"
    ));
    let main_build = meta["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|module| module["name"].as_str() == Some("main"))
        .and_then(|module| module["build_id"].as_str())
        .unwrap_or("");
    lines.push_str(&format!(
        "const char hos_main_build_id[] = \"{main_build}\";\n"
    ));
    for symbol in [
        "_init_libc0",
        "_init_libc1",
        "_init_libc2",
        "nnosInitialize",
        "nninitInitializeSdkModule",
        "nvnBootstrapLoaderInternal",
    ] {
        lines.push_str(&format!(
            "const uint64_t hos_entry_{} = {:#x}ULL;\n",
            symbol.trim_start_matches('_'),
            exports(symbol)
        ));
    }
    let hooks = thread_hooks(image, meta);
    lines.push_str(&format!(
        "const uint64_t hos_thread_start_hook = {:#x}ULL, hos_thread_exit_hook = {:#x}ULL;\n",
        hooks[0], hooks[1]
    ));
    fs::write(out_dir.join("hos_image.c"), lines).map_err(|e| e.to_string())?;
    Ok(hooks)
}

/// Write the three standalone lift files into `gen`; `host` defaults to the
/// repository's host folder.
pub fn write_json(exefs: &str, out: &str, host: &str) -> Result<Value, String> {
    let exefs = Path::new(exefs);
    let out_dir = Path::new(out);
    let host = if host.is_empty() {
        crate::kit::root().join("host")
    } else {
        Path::new(host).to_path_buf()
    };
    fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let implemented = implemented(&host)?;
    let modules = crate::nso_image::load_exefs(exefs)?;
    let nvn = nvn_exports(&modules, &implemented);
    let (mut hle, stubbed) = hle_set(&modules, &implemented);
    hle.extend(nvn.iter().cloned());
    let hle_count = hle.len();
    let mut all: Vec<String> = hle.into_iter().collect();
    all.extend(stubbed.iter().cloned());
    all.sort();
    let image = Image::build(exefs, 0, false, &all, &[])?;
    let meta = image.json();
    let stubs = gen_stubs(out_dir, &meta, &implemented)?;
    write_nvn_exports(out_dir, &nvn)?;
    let hooks = write_image_c(out_dir, &image, &meta, exefs)?;
    Ok(json!({
        "hle": hle_count,
        "stubbed": stubbed.len(),
        "stubs": stubs,
        "nvn": nvn.len(),
        "thread_start": hooks[0],
        "thread_exit": hooks[1],
        "files": ["a64_stubs.c", "nvn2_exports.c", "hos_image.c"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn itanium_names_are_read() {
        assert_eq!(
            nested_name("_ZN2nn3hid4InitEv").as_deref(),
            Some(&["nn", "hid", "Init"][..])
        );
        assert_eq!(
            nested_name("_ZN2nn6camera3sdk12CreateCameraEv").as_deref(),
            Some(&["nn", "camera", "sdk", "CreateCamera"][..])
        );
        assert!(nested_name("_ZN2nn2fs6FileC1Ev").is_some());
        assert!(nested_name("_Z3foov").is_none());
        assert!(nested_name("main").is_none());
    }

    #[test]
    fn os_facing_names_are_recognised() {
        assert!(os_facing("_ZN2nn3hid4InitEv"));
        assert!(os_facing("_ZN2nn6camera3sdk12CreateCameraEv"));
        assert!(!os_facing("_ZN2nn2fs6FileC1Ev"));
        assert!(!os_facing("_ZN2nn2fs6FileC1Ev"));
    }

    #[test]
    fn nvn_names_are_recognised() {
        assert_eq!(
            nvn_proc_name("nvnDeviceGetProcAddress").as_deref(),
            Some("nvnDeviceGetProcAddress")
        );
        assert_eq!(
            nvn_proc_name("_Z24nvnNativeImageInitializePv").as_deref(),
            Some("nvnNativeImageInitialize")
        );
        assert_eq!(nvn_proc_name("_ZN2nn3hid4InitEv"), None);
    }
}
