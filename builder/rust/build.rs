// The vendored ASTC encoder (astcenc/): ARM's astc-encoder, built through a
// two-function C wrapper. No bindgen or libclang is needed.
fn main() {
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .include("astcenc")
        .file("astcenc/astcenc_shim.cpp");
    for source in [
        "astcenc_averages_and_directions",
        "astcenc_block_sizes",
        "astcenc_color_quantize",
        "astcenc_color_unquantize",
        "astcenc_compress_symbolic",
        "astcenc_compute_variance",
        "astcenc_decompress_symbolic",
        "astcenc_diagnostic_trace",
        "astcenc_entry",
        "astcenc_find_best_partitioning",
        "astcenc_ideal_endpoints_and_weights",
        "astcenc_image",
        "astcenc_integer_sequence",
        "astcenc_mathlib",
        "astcenc_mathlib_softfloat",
        "astcenc_partition_tables",
        "astcenc_percentile_tables",
        "astcenc_pick_best_endpoint_format",
        "astcenc_quantization",
        "astcenc_symbolic_physical",
        "astcenc_weight_align",
        "astcenc_weight_quant_xfer_tables",
    ] {
        build.file(format!("astcenc/{source}.cpp"));
    }
    build.compile("hookshot_astcenc");
    println!("cargo:rerun-if-changed=astcenc");
    if std::env::var("CARGO_CFG_TARGET_ENV").ok().as_deref() == Some("gnu") {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
