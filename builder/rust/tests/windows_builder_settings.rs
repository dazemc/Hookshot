// The Windows builder's accepted-license record (upstream NotRexed/Hookshot b56bf07c, #2528): the three builders this
// tree keeps must save NVIDIA's acceptance in their settings and read it when an update decides whether to keep a
// folder's DLSS. Source checks only; no builder UI, game folder or game data is involved.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn source(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn the_wpf_builder_keeps_the_record_not_the_sdk_files() {
    let settings = source("builder/HookshotBuilder/BuilderSettings.cs");
    assert!(
        settings.contains("public bool DlssLicense"),
        "builder-settings.json needs a DlssLicense field"
    );
    let window = source("builder/HookshotBuilder/MainWindow.xaml.cs");
    assert!(window.contains("dlssLicense = s.DlssLicense;"));
    assert!(window.contains("DlssLicense = dlssLicense"));
    assert!(
        window.contains("dlssLicense = true;"),
        "accepting the license must set the record"
    );
    assert!(
        window.contains("saved?.DlssLicense == true"),
        "an update must keep DLSS from the record when the SDK is gone"
    );
    assert!(
        window.contains("File.Exists(System.IO.Path.Combine(Storage.NgxSdk, \"LICENSE.txt\"))"),
        "the SDK's file stays the record for a license accepted before this change"
    );
    assert!(
        window.contains("nvngx_dlssg.dll"),
        "the notice must name DLSS Frame Generation's runtime too (upstream 55b976c0)"
    );
}

#[test]
fn the_python_builder_keeps_the_record_not_the_sdk_files() {
    let window = source("builder/py/hookshot_builder/window.py");
    assert!(window.contains("self.dlss_license = bool(s.get(\"DlssLicense\"))"));
    assert!(window.contains("\"DlssLicense\": getattr(self, \"dlss_license\", False)"));
    assert!(
        window.contains("self.dlss_license = True"),
        "accepting the license must set the record"
    );
    assert!(
        window.contains("core.load_settings().get(\"DlssLicense\")"),
        "an update must keep DLSS from the record when the SDK is gone"
    );
    assert!(
        window.contains("os.path.isfile(os.path.join(core.ngx_sdk(), \"LICENSE.txt\"))"),
        "the SDK's file stays the record for a license accepted before this change"
    );
    assert!(
        window.contains("nvngx_dlssg.dll"),
        "the notice must name DLSS Frame Generation's runtime too (upstream 55b976c0)"
    );
}

#[test]
fn the_flutter_builder_keeps_the_record_not_the_sdk_files() {
    let record = source("builder/flutter/lib/updater/dlss.dart");
    assert!(record.contains("config['dlss_license'] == '1'"));
    assert!(record.contains("config['dlss'] == '1'"));
    assert!(record.contains("next['dlss_license'] = '1'"));
    assert!(record.contains("keepDlssOnUpdate("));
    assert!(record.contains("hasRuntime && (licenseAccepted || sdkLicense)"));
    let bloc = source("builder/flutter/lib/blocs/builder_bloc.dart");
    assert!(bloc.contains("dlss.dlssAfterToggle(_cfg, e.value)"));
    assert!(bloc.contains("dlss.dlssLicenseAccepted(_cfg)"));
    assert!(bloc.contains("updateDlss("));
    assert!(bloc.contains("ticked: s is BuilderForm && s.dlss"));
    assert!(bloc.contains("toggled: s is BuilderForm && s.dlssToggled"));
    let config = source("builder/flutter/lib/config.dart");
    assert!(config.contains("nvngx_dlssg.dll"));
}
