// The small C entry the Rust prompt build uses: ASTC 4x4 LDR/LDR-sRGB from RGBA8.
// The encoder itself is ARM's astc-encoder (Apache-2.0, LICENSE.txt), vendored
// here so no bindgen or libclang is needed at build time.

#include "astcenc.h"

namespace {

// (context allocation is the expensive part of an encode: the prompt build
// compresses hundreds of small textures with the same settings. astcenc
// documents that a single-threaded context may be reused, and
// astcenc_compress_image resets it itself, so keep one per thread and
// profile.)
struct Context {
    astcenc_context* context = nullptr;

    ~Context() {
        if (context) astcenc_context_free(context);
    }

    astcenc_error get(astcenc_profile profile, float preset, unsigned int flags) {
        if (context) return ASTCENC_SUCCESS;
        astcenc_config config;
        astcenc_error error = astcenc_config_init(profile, 4, 4, 1, preset, flags, &config);
        if (error != ASTCENC_SUCCESS) return error;
        return astcenc_context_alloc(&config, 1, &context);
    }
};

struct Contexts {
    Context compress_ldr;
    Context compress_srgb;
    Context decompress_ldr;
    Context decompress_srgb;
};

Contexts& contexts() {
    thread_local Contexts cached;
    return cached;
}

} // namespace

extern "C" int hookshot_astc_compress(const unsigned char* rgba, int width, int height, int srgb,
                                      unsigned char* out, unsigned long long out_size) {
    const astcenc_profile profile = srgb ? ASTCENC_PRF_LDR_SRGB : ASTCENC_PRF_LDR;
    Context& cached = srgb ? contexts().compress_srgb : contexts().compress_ldr;
    astcenc_error error = cached.get(profile, ASTCENC_PRE_THOROUGH, 0);
    if (error != ASTCENC_SUCCESS) return (int)error;
    void* slice = const_cast<unsigned char*>(rgba);
    astcenc_image image{};
    image.dim_x = (unsigned)width;
    image.dim_y = (unsigned)height;
    image.dim_z = 1;
    image.data_type = ASTCENC_TYPE_U8;
    image.data = &slice;
    const astcenc_swizzle swizzle{ASTCENC_SWZ_R, ASTCENC_SWZ_G, ASTCENC_SWZ_B, ASTCENC_SWZ_A};
    error = astcenc_compress_image(cached.context, &image, &swizzle, out, (size_t)out_size, 0);
    return (int)error;
}

extern "C" int hookshot_astc_decompress(const unsigned char* blocks, int width, int height, int srgb,
                                        unsigned char* rgba, unsigned long long rgba_size) {
    const astcenc_profile profile = srgb ? ASTCENC_PRF_LDR_SRGB : ASTCENC_PRF_LDR;
    Context& cached = srgb ? contexts().decompress_srgb : contexts().decompress_ldr;
    astcenc_error error = cached.get(profile, ASTCENC_PRE_MEDIUM, ASTCENC_FLG_DECOMPRESS_ONLY);
    if (error != ASTCENC_SUCCESS) return (int)error;
    void* slice = rgba;
    astcenc_image image{};
    image.dim_x = (unsigned)width;
    image.dim_y = (unsigned)height;
    image.dim_z = 1;
    image.data_type = ASTCENC_TYPE_U8;
    image.data = &slice;
    const astcenc_swizzle swizzle{ASTCENC_SWZ_R, ASTCENC_SWZ_G, ASTCENC_SWZ_B, ASTCENC_SWZ_A};
    error = astcenc_decompress_image(cached.context, blocks, (size_t)((width + 3) / 4 * ((height + 3) / 4) * 16), &image, &swizzle, 0);
    return (int)error;
}
