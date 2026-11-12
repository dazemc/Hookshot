// Game-free checks of builder/linux/linux_update.py and the Python Windows
// setup's archive gate: a requested release's list must name that release, a
// part's zip cannot name a path outside its folder, and a failed update leaves
// the installed version and its current pointer alone. The real modules are
// imported and driven with authored bytes; no network, release or game is
// involved.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "hookshot-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

fn python() -> Option<String> {
    for candidate in ["python3", "python"] {
        if Command::new(candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }
    None
}

#[test]
fn linux_update_refuses_another_release_and_unsafe_archive_names() {
    let Some(python) = python() else {
        eprintln!("skipping the Linux updater checks: no Python interpreter");
        return;
    };
    let dir = scratch("linux-update-safety");
    let script = dir.join("check.py");
    let source = root().join("builder/linux/linux_update.py");
    let script_text = format!(
        r##"
import hashlib, importlib.util, io, os, sys, tempfile, zipfile
sys.path.insert(0, os.path.dirname({source:?}))
spec = importlib.util.spec_from_file_location("linux_update", {source:?})
lu = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lu)

sha = hashlib.sha256(b"x").hexdigest()
def get(url):
    return ("tag v2.2.0\npart %s 1 . https://e/a\ntree\n" % sha).encode()
try:
    lu.fetch_manifest(get, tag="v2.1.0")
    raise SystemExit("a list for v2.2.0 was accepted as v2.1.0")
except ValueError:
    pass
assert lu.fetch_manifest(get, tag="v2.2.0").tag == "v2.2.0"
try:
    lu.list_url("../outside")
    raise SystemExit("an unsafe tag made a URL")
except ValueError:
    pass

def zip_bytes(name, link=None):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        if name != "usr/share/hookshot/builder/linux/AppRun":
            if link is None:
                z.writestr(name, "escaped")
            else:
                info = zipfile.ZipInfo(name)
                info.external_attr = (0o120777 << 16)
                z.writestr(info, link)
        z.writestr("usr/share/hookshot/builder/linux/AppRun", "#!/bin/sh\n")
    return buf.getvalue()

for name in ["../evil", "..\\evil", "C:/evil", "a//evil", "/abs"]:
    with tempfile.TemporaryDirectory() as tmp:
        stage = os.path.join(tmp, "stage")
        try:
            lu._unpack(zip_bytes(name), stage, None, False, stage)
            raise SystemExit("unsafe member accepted: " + name)
        except ValueError:
            pass
        assert not os.path.exists(os.path.join(tmp, "evil"))

with tempfile.TemporaryDirectory() as tmp:
    stage = os.path.join(tmp, "stage")
    try:
        lu._unpack(zip_bytes("link", "../outside"), stage, None, False, stage)
        raise SystemExit("a symlink leaving staging was accepted")
    except ValueError:
        pass

root = tempfile.mkdtemp()
os.environ["HOOKSHOT_BUILDER_HOME"] = root
good = zip_bytes("usr/share/hookshot/builder/linux/AppRun")
good_manifest = lu.parse_list(
    "tag v1.0.0\npart %s %d . https://e/good\ntree\n" % (hashlib.sha256(good).hexdigest(), len(good)))
lu.install(good_manifest, None, None, get=lambda url: good)
installed = os.path.join(lu.data_root(), "versions", "v1.0.0", "usr", "share", "hookshot", "builder", "linux", "AppRun")
assert os.path.isfile(installed)
bad = lu.parse_list("tag v2.0.0\npart %s 11 . https://e/bad\ntree\n" % hashlib.sha256(b"bad").hexdigest())
try:
    lu.install(bad, None, None, get=lambda url: b"wrong bytes")
    raise SystemExit("a bad download installed")
except RuntimeError:
    pass
assert open(os.path.join(lu.data_root(), "current")).read().strip() == "v1.0.0"
assert os.path.isfile(installed)
assert not os.path.exists(os.path.join(lu.data_root(), "versions", "v2.0.0"))

sys.path.insert(0, os.path.join(os.path.dirname({source:?}), "..", "py"))
import hookshot_builder.setup_parts as sp

def check_zip(name, mode=0o100644):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        info = zipfile.ZipInfo(name)
        info.external_attr = (mode << 16)
        z.writestr(info, "x")
    with zipfile.ZipFile(io.BytesIO(buf.getvalue())) as z:
        sp.check_zip_members(z)

check_zip("HookshotBuilder/tools/x")
for name in ["../evil", "..\\\\evil", "C:/evil", "a//evil", "/abs"]:
    try:
        check_zip(name)
        raise SystemExit("the Python setup accepted a zip member: " + name)
    except sp.SetupError:
        pass
try:
    check_zip("link", 0o120777)
    raise SystemExit("the Python setup accepted a zip link")
except sp.SetupError:
    pass
print("ok")
"##,
        source = source.display().to_string(),
    );
    fs::write(&script, script_text).expect("write Python fixture");
    let run = Command::new(python)
        .arg("-B")
        .arg(&script)
        .output()
        .expect("run the Python fixture");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "ok");
    let _ = fs::remove_dir_all(dir);
}
