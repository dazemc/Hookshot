// The small C entry the Rust prompt build uses: ASTC 4x4 LDR/LDR-sRGB from RGBA8.
// The encoder itself is ARM's astc-encoder (Apache-2.0, LICENSE.txt), vendored
// here so no bindgen or libclang is needed at build time.
#include "astcenc.h"

extern "C" int hookshot_astc_compress(const unsigned char* rgba, int width, int height, int srgb,
                                      unsigned char* out, unsigned long long out_size) {
    astcenc_config config;
    const astcenc_profile profile = srgb ? ASTCENC_PRF_LDR_SRGB : ASTCENC_PRF_LDR;
    astcenc_error error = astcenc_config_init(profile, 4, 4, 1, ASTCENC_PRE_THOROUGH, 0, &config);
    if (error != ASTCENC_SUCCESS) return (int)error;
    astcenc_context* context = nullptr;
    error = astcenc_context_alloc(&config, 1, &context);
    if (error != ASTCENC_SUCCESS) return (int)error;
    void* slice = const_cast<unsigned char*>(rgba);
    astcenc_image image{};
    image.dim_x = (unsigned)width;
    image.dim_y = (unsigned)height;
    image.dim_z = 1;
    image.data_type = ASTCENC_TYPE_U8;
    image.data = &slice;
    const astcenc_swizzle swizzle{ASTCENC_SWZ_R, ASTCENC_SWZ_G, ASTCENC_SWZ_B, ASTCENC_SWZ_A};
    error = astcenc_compress_image(context, &image, &swizzle, out, (size_t)out_size, 0);
    astcenc_context_free(context);
    return (int)error;
}

extern "C" int hookshot_astc_decompress(const unsigned char* blocks, int width, int height, int srgb,
                                        unsigned char* rgba, unsigned long long rgba_size) {
    astcenc_config config;
    const astcenc_profile profile = srgb ? ASTCENC_PRF_LDR_SRGB : ASTCENC_PRF_LDR;
    astcenc_error error = astcenc_config_init(profile, 4, 4, 1, ASTCENC_PRE_MEDIUM, ASTCENC_FLG_DECOMPRESS_ONLY, &config);
    if (error != ASTCENC_SUCCESS) return (int)error;
    astcenc_context* context = nullptr;
    error = astcenc_context_alloc(&config, 1, &context);
    if (error != ASTCENC_SUCCESS) return (int)error;
    void* slice = rgba;
    astcenc_image image{};
    image.dim_x = (unsigned)width;
    image.dim_y = (unsigned)height;
    image.dim_z = 1;
    image.data_type = ASTCENC_TYPE_U8;
    image.data = &slice;
    const astcenc_swizzle swizzle{ASTCENC_SWZ_R, ASTCENC_SWZ_G, ASTCENC_SWZ_B, ASTCENC_SWZ_A};
    error = astcenc_decompress_image(context, blocks, (size_t)((width + 3) / 4 * ((height + 3) / 4) * 16), &image, &swizzle, 0);
    astcenc_context_free(context);
    return (int)error;
}
