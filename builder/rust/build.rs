use std::path::Path;

fn main() {
    // The ZBIC decompressor is the game's zstd variant: compile the vendored
    // single-file zstd with support for it into the engine, so the tool works
    // on every platform without a separately built zbic library.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/zbic");
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
