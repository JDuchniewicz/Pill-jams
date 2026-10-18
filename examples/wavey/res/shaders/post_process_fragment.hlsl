// Post-process fragment shader (pill_assets regenerates the .wgsl).
// A single pass composed of three independent sections, each driven by its own parameters
// (mirrored by PostProcessParams in post_process.rs and edited in the egui "Post processing"
// window):
//   1. contrast & gamma
//   2. chromatic aberration
//   3. vignette
// The source is the composed scene color (the HDR target) and the result feeds the engine's
// tonemap pass, so the effects run on the linear scene color, before tonemapping.

[[vk::binding(0, 0)]] Texture2D    scene_texture;
[[vk::binding(1, 0)]] SamplerState scene_sampler;

struct PostProcessParams {
    float4 contrast_gamma; // x = contrast, y = gamma, z = chromatic aberration strength
    float4 vignette;       // x = strength, y = softness, z = radius
    float4 screen;         // xy = render resolution in pixels
};
[[vk::binding(2, 0)]] ConstantBuffer<PostProcessParams> params;

/// Aspect-corrected distance from the screen centre, 0 at the centre and 1 at the corners.
float screen_radius(float2 uv, float2 aspect) {
    float2 centered = uv - 0.5;
    float max_radius = length(0.5 * aspect);
    return length(centered * aspect) / max(max_radius, 0.0001);
}

[shader("fragment")]
float4 fs_main(float2 texture_coordinates : TEXCOORD0) : SV_Target {
    float2 uv = texture_coordinates;
    float2 aspect = float2(params.screen.x / max(params.screen.y, 1.0), 1.0);

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

    return float4(color, 1.0);
}
