// Game-free source checks for the HDR scene format: the temporal upscalers must take the RGBA16F scene buffer that the
// PsychoV35 tone mapper produces (hos_scene_rgba16f), not only the game's usual B10G11R11. Native HDR with the row on
// Vanilla keeps R11G11B10F for the upscalers; with PsychoV35 the scene is RGBA16F, and the upscalers must accept it or
// the game's own upscale runs instead. No game, dump or GPU is involved.
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative)).expect(relative)
}

#[test]
fn temporal_scene_format_accepts_hdr_and_ldr() {
    let header = read("gpu/vk/vk_temporal.h");
    assert!(
        header.contains("temporal_scene_format"),
        "vk_temporal.h must declare temporal_scene_format"
    );
    assert!(
        header.contains("VK_FORMAT_B10G11R11_UFLOAT_PACK32") && header.contains("VK_FORMAT_R16G16B16A16_SFLOAT"),
        "temporal_scene_format must accept both the LDR scene and the HDR (RGBA16F) scene"
    );

    // The temporal chain's scene camera, stretch and upscale gates use the helper instead of a hard-coded B10G11R11 check.
    let temporal = read("gpu/vk/vk_temporal.cpp");
    assert!(temporal.contains("temporal_scene_format(color0->format)"));
    assert!(temporal.contains("temporal_scene_format(out->format)"));
    assert!(temporal.contains("temporal_scene_format(in->format)"));

    // FSR 3.1's own path, including its motion/depth inputs, takes the HDR scene.
    let fsr3 = read("gpu/vk/vk_fsr3.cpp");
    let fsr3_inputs = read("gpu/vk/vk_fsr3_inputs.cpp");
    assert!(fsr3.contains("temporal_scene_format(in->format)"));
    assert!(fsr3.contains("temporal_scene_format(out->format)"));
    assert!(fsr3_inputs.contains("temporal_scene_format(color0->format)"));

    // The scene-detection gates in vk_draw.cpp accept the HDR scene.
    let draw = read("gpu/vk/vk_draw.cpp");
    assert!(draw.contains("temporal_scene_format(at.color_format[0])"));

    // The reactive mask accepts the HDR scene too.
    let reactive = read("gpu/vk/vk_reactive.cpp");
    assert!(reactive.contains("temporal_scene_format(color->format)"));

    // The prep passes are format-agnostic: they sample the scene through a sampler2D, so the RGBA16F (HDR) scene needs no
    // shader variant; clamping to 0..1 is correct for the perceptual (square-root) encoding in both formats.
    let fsr3_prep = read("gpu/vk/shaders/fsr3_prep.comp");
    let dlss_prep = read("gpu/vk/shaders/dlss_prep.comp");
    assert!(fsr3_prep.contains("layout(binding = 0) uniform sampler2D scene;"));
    assert!(dlss_prep.contains("layout(binding = 0) uniform sampler2D scene;"));
}
