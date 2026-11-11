use std::path::Path;

fn main() {
    // Atmosphere's single-file zstd with ZBIC support. It lives in its own
    // shared library because its frame format replaces standard zstd's magic:
    // the engine uses the normal zstd crate for packages and this one only for
    // the game's ZBIC NSO segments.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/zbic");
    cc::Build::new()
        .file(root.join("zstd.c"))
        .file(root.join("zbic_api.c"))
        .define("ZSTD_ZBIC_SUPPORT", "1")
        .include(&root)
        .warnings(false)
        .opt_level(2)
        .compile("zbic");
    println!("cargo:rerun-if-changed={}", root.join("zstd.c").display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("zbic_api.c").display()
    );
    println!("cargo:rerun-if-changed={}", root.join("zstd.h").display());
}
