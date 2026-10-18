// Post-process fragment shader (pill_assets regenerates the .wgsl).
// A single pass composed of four independent sections, each driven by its own parameters
// (mirrored by PostProcessParams in post_process.rs and edited in the egui "Post processing"
// window):
//   1. contrast & gamma
//   2. chromatic aberration
//   3. vignette
//   4. dithering
// The source is the composed scene color (the HDR target) and the result feeds the engine's
// tonemap pass, so the effects run on the linear scene color, before tonemapping.

[[vk::binding(0, 0)]] Texture2D    scene_texture;
[[vk::binding(1, 0)]] SamplerState scene_sampler;

struct PostProcessParams {
    float4 contrast_gamma; // x = contrast, y = gamma, z = chromatic aberration strength
    float4 vignette;       // x = strength, y = softness, z = radius
    float4 screen;         // xy = render resolution in pixels
    float4 dither;         // x = strength, y = quantization levels, z = pattern scale in pixels,
                           // w = pattern index (0 = interleaved gradient, 1 = Bayer 4x4,
                           //     2 = Bayer 8x8, 3 = Bayer 16x16, 4 = white noise,
                           //     5 = stripes, 6 = checkerboard, 7 = halftone)
    float4 dither_extra;   // x = pattern angle (radians), y = line width / dot size,
                           // z = edge softness
};
[[vk::binding(2, 0)]] ConstantBuffer<PostProcessParams> params;

/// Aspect-corrected distance from the screen centre, 0 at the centre and 1 at the corners.
float screen_radius(float2 uv, float2 aspect) {
    float2 centered = uv - 0.5;
    float max_radius = length(0.5 * aspect);
    return length(centered * aspect) / max(max_radius, 0.0001);
}

/// Interleaved gradient noise: a cheap, stable per-pixel dither pattern in [0, 1).
float interleaved_gradient_noise(float2 pixel) {
    return frac(52.9829189 * frac(dot(pixel, float2(0.06711056, 0.00583715))));
}

/// 2x2 Bayer building block, values in [0, 1).
float bayer2(float2 a) {
    a = floor(a);
    return frac(a.x * 0.5 + a.y * a.y * 0.75);
}

/// 4x4 Bayer ordered matrix, values in [0, 1).
float bayer4(float2 a) {
    return bayer2(a * 0.5) * 0.25 + bayer2(a);
}

/// 8x8 Bayer ordered matrix, values in [0, 1).
float bayer8(float2 a) {
    return bayer4(a * 0.5) * 0.25 + bayer2(a);
}

/// 16x16 Bayer ordered matrix, values in [0, 1).
float bayer16(float2 a) {
    return bayer8(a * 0.5) * 0.25 + bayer2(a);
}

/// Per-pixel white-noise hash in [0, 1).
float white_noise(float2 pixel) {
    return frac(sin(dot(pixel, float2(12.9898, 78.233))) * 43758.5453);
}

/// Linear RGB -> sRGB display encoding.
float3 linear_to_srgb(float3 c) {
    c = saturate(c);
    float3 low = c * 12.92;
    float3 high = 1.055 * pow(c, 1.0 / 2.4) - 0.055;
    return lerp(high, low, step(c, 0.0031308));
}

/// sRGB display encoding -> linear RGB.
float3 srgb_to_linear(float3 c) {
    c = saturate(c);
    float3 low = c / 12.92;
    float3 high = pow((c + 0.055) / 1.055, 2.4);
    return lerp(high, low, step(c, 0.04045));
}

/// Rotates a 2D point around the origin.
float2 rotate2(float2 p, float angle) {
    float s = sin(angle);
    float c = cos(angle);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

/// Soft-edged diagonal stripes; `width` is the bright fraction of a pattern cell.
float stripe_pattern(float2 pixel, float angle, float width, float softness) {
    float2 p = rotate2(pixel, angle);
    float triangle = 1.0 - abs(2.0 * frac(p.x) - 1.0);
    float threshold = 1.0 - saturate(width);
    return smoothstep(threshold - softness, threshold + softness, triangle);
}

/// Soft-edged halftone dots on a rotated grid; `size` is the dot diameter in cell units.
float halftone_pattern(float2 pixel, float angle, float size, float softness) {
    float2 p = rotate2(pixel, angle);
    float2 cell = frac(p) - 0.5;
    float radius = 0.5 * saturate(size);
    float edge = max(softness, 0.0001) * 0.25;
    return 1.0 - smoothstep(radius - edge, radius + edge, length(cell));
}

[shader("fragment")]
float4 fs_main(float2 texture_coordinates : TEXCOORD0, float4 sv_position : SV_POSITION) : SV_Target {
    float2 aspect = float2(params.screen.x / max(params.screen.y, 1.0), 1.0);

    // Every dither cell is rendered as one flat block: the scene is sampled and shaded once at
    // the cell centre, so all pixels inside the cell share the same quantized palette colour.
    float cell_size = max(params.dither.z, 1.0);
    float2 cell = floor(sv_position.xy / cell_size);
    float2 uv = (cell + 0.5) * cell_size / max(params.screen.xy, 1.0);

    // --- Section 2: chromatic aberration -------------------------------------------------
    // The red and blue channels are sampled at slightly different distances from the centre,
    // which fringes the edges of the screen. Strength 0 = off.
    float2 centered = uv - 0.5;
    float2 offset = centered * aspect * params.contrast_gamma.z;
    float3 color;
    color.r = scene_texture.Sample(scene_sampler, uv + offset).r;
    color.g = scene_texture.Sample(scene_sampler, uv).g;
    color.b = scene_texture.Sample(scene_sampler, uv - offset).b;

    // --- Section 1: contrast & gamma -----------------------------------------------------
    // Contrast pivots around mid grey, gamma is applied as a power curve (1 = unchanged).
    color = (color - 0.5) * params.contrast_gamma.x + 0.5;
    color = max(color, 0.0);
    color = pow(color, 1.0 / max(params.contrast_gamma.y, 0.0001));

    // --- Section 3: vignette -------------------------------------------------------------
    // Darkens the corners: 0 inside `radius`, ramping up over `softness`, scaled by `strength`.
    float radius = screen_radius(uv, aspect);
    float edge = smoothstep(
        params.vignette.z,
        params.vignette.z + max(params.vignette.y, 0.0001),
        radius
    );
    color *= 1.0 - params.vignette.x * edge;

    // --- Section 4: dithering ------------------------------------------------------------
    // The pattern picks between the nearest quantized colours, so the image itself is rebuilt
    // from a reduced palette. The whole step runs in display (sRGB) space: quantizing the linear
    // HDR value before the output sRGB transform would mostly add a visible pattern on top of the
    // image instead of changing the pixels that are displayed.
    float levels = max(params.dither.y, 2.0);
    uint pattern = (uint)params.dither.w;
    float angle = params.dither_extra.x;
    float width = params.dither_extra.y;
    float softness = params.dither_extra.z;
    float dither_noise;
    if (pattern == 1u) {
        dither_noise = bayer4(cell);
    } else if (pattern == 2u) {
        dither_noise = bayer8(cell);
    } else if (pattern == 3u) {
        dither_noise = bayer16(cell);
    } else if (pattern == 4u) {
        dither_noise = white_noise(cell);
    } else if (pattern == 5u) {
        dither_noise = stripe_pattern(cell, angle, width, softness);
    } else if (pattern == 6u) {
        dither_noise = fmod(floor(cell.x) + floor(cell.y), 2.0);
    } else if (pattern == 7u) {
        dither_noise = halftone_pattern(cell, angle, width, softness);
    } else {
        dither_noise = interleaved_gradient_noise(cell);
    }
    dither_noise -= 0.5;

    float3 display_color = linear_to_srgb(color);
    display_color += dither_noise * (params.dither.x / levels);
    display_color = floor(display_color * levels + 0.5) / levels;
    color = srgb_to_linear(display_color);

    return float4(color, 1.0);
}
