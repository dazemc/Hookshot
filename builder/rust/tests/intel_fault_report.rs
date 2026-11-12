// The Intel (and AMD) device-fault report on a lost device (gpu/vk/vk_draw.cpp device_fault_report, vk_replay.cpp
// device_lost; upstream NotRexed/Hookshot a435efd0 #2603 and ee64a963 #2117). Intel's iGPUs fault through
// VK_EXT_device_fault without NVIDIA's checkpoints, and the game used to end with VK_ERROR_DEVICE_LOST and no address.
// A source check: the function reads the extension, the report runs from device_lost()'s one report, and it is automatic
// exactly where there is nothing else to read (no checkpoints, no buffer markers). On NVIDIA it stays behind
// RX_VK_FAULT_INFO, because vkGetDeviceFaultInfoEXT can hang after a fault there.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|_| panic!("missing {path}"))
}

/// A function's definition from a source, braces matched.
fn block(source: &str, marker: &str) -> String {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing {marker}"));
    let mut end = start + source[start..].find('{').expect("opening brace") + 1;
    let bytes = source.as_bytes();
    let mut depth = 1;
    while depth > 0 {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        end += 1;
    }
    source[start..end].to_string()
}

#[test]
fn a_lost_device_reports_the_fault_without_nvidias_checkpoints() {
    let draw = read("gpu/vk/vk_draw.cpp");
    let replay = read("gpu/vk/vk_replay.cpp");
    let main = read("gpu/vk/vk_main.cpp");
    let internal = read("gpu/vk/vk_internal.h");

    // The report itself: the extension's counts, then the address and vendor details and the driver's description.
    let report = block(&draw, "void device_fault_report() {");
    for part in [
        "vkGetDeviceFaultInfoEXT(dev.device, &counts, nullptr)",
        "VkDeviceFaultCountsEXT",
        "VkDeviceFaultAddressInfoEXT",
        "VkDeviceFaultVendorInfoEXT",
        "info.description",
        "read invalid",
        "write invalid",
        "execute invalid",
        "instruction pointer fault",
        "vendorFaultCode",
        "vendorFaultData",
    ] {
        assert!(report.contains(part), "device_fault_report lacks {part}");
    }

    // It is reached from device_lost()'s one report, and only from there.
    let lost = block(&replay, "[[noreturn]] void device_lost(const char* why) {");
    assert!(
        lost.contains("std::call_once(once, checkpoints_report)"),
        "device_lost no longer runs the one checkpoints/fault report"
    );
    assert!(
        lost.contains("VK_ERROR_DEVICE_LOST"),
        "device_lost lost its VK_ERROR_DEVICE_LOST message"
    );
    assert_eq!(
        draw.matches("device_fault_report()").count(),
        3,
        "device_fault_report should be defined and called from checkpoints_report twice"
    );
    for file in [
        "gpu/vk/vk_replay.cpp",
        "gpu/vk/vk_main.cpp",
        "gpu/vk/vk_internal.h",
        "host/rx_backend.c",
    ] {
        assert!(
            !read(file).contains("device_fault_report()"),
            "{file} calls device_fault_report outside checkpoints_report"
        );
    }

    // The automatic read where neither NVIDIA's checkpoints nor the buffer markers can name the fault: AMD's RADV and
    // Intel. Where they can, the report stays behind RX_VK_FAULT_INFO (the NVIDIA hang).
    let checkpoints = block(&draw, "void checkpoints_report() {");
    assert!(
        checkpoints.contains("const bool fault_auto = device_fault_query::automatic("),
        "the fault report no longer decides whether it is automatic"
    );
    assert!(
        checkpoints.contains("struct FaultLast {")
            && checkpoints.contains("~FaultLast() { if (wanted) device_fault_report(); }")
            && checkpoints.contains("} fault_last{fault_forced || fault_auto};"),
        "the fault report is not automatic where there is nothing else to read, or lost its RX_VK_FAULT_INFO guard"
    );
    assert!(
        checkpoints.contains("if (fault_forced) device_fault_report();"),
        "the opt-in early fault read is gone"
    );
    assert!(
        checkpoints.contains("dev.checkpoints ? \"the checkpoints\"")
            && checkpoints.contains("marker_words ? \"the buffer markers\"")
            && checkpoints.contains("\"the fault info (RX_VK_CHECKPOINTS=1 for markers)\""),
        "the lost-device log no longer says which report it reads"
    );

    // The extension is asked for at device creation, or the report would always say it is unavailable.
    assert!(
        internal.contains("device_fault = false"),
        "the device's device_fault flag is gone"
    );
    assert!(
        main.contains("optional(VK_EXT_DEVICE_FAULT_EXTENSION_NAME, dev.device_fault)"),
        "VK_EXT_device_fault is not enabled"
    );
    assert!(
        main.contains("VkPhysicalDeviceFaultFeaturesEXT fault{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FAULT_FEATURES_EXT}"),
        "VK_EXT_device_fault's features are not chained"
    );
    assert!(
        main.contains("if (dev.device_fault) *tail = &fault, tail = &fault.pNext;"),
        "VK_EXT_device_fault's features are not enabled when the device has the extension"
    );
}
