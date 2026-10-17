// Water mesh fragment shader — the "water_mesh_shader" material (pill_assets generates the .wgsl).
// The water is shaded with a world-space vertical gradient, overlaid with a light highlight
// gradient where the (inverted) depth value is above a threshold — i.e. near the surface.
// An egui checkbox can switch to a debug view that visualises the second UV set instead.

struct SurfaceParams {
    uint  count;     // number of surface samples
    float x_min;     // world X of the first sample
    float x_max;     // world X of the last sample
    float max_depth; // depth mapped to white
    uint  uv2_view;  // non-zero: visualise the second UV set instead of the water shading
    uint  pad0;
    uint  pad1;
    uint  pad2;
};
[[vk::binding(2, 0)]] ConstantBuffer<SurfaceParams> surface_params;
// Water surface height (world Y) per sample, uniformly spaced between x_min and x_max.
[[vk::binding(3, 0)]] StructuredBuffer<float> surface_heights;

// Water shading colours ("Water look" panel)
struct WaterLook {
    float4 top_color;        // rgb = water colour at the top of the world gradient
    float4 bottom_color;     // rgb = water colour at the bottom
    float4 highlight_top;    // rgb = overlay colour at the top of the gradient
    float4 highlight_bottom; // rgb = overlay colour at the bottom
    float4 params;           // x = highlight threshold, y = gradient top Y, z = gradient bottom Y
};
[[vk::binding(4, 0)]] ConstantBuffer<WaterLook> water_look;

struct FragmentInput {
    [[vk::location(0)]] float2 uv       : TEXCOORD0;
    [[vk::location(1)]] float  world_x  : TEXCOORD1;
    [[vk::location(2)]] float  world_y  : TEXCOORD2;
    // Second UV set (vertical velocity delta); available for effects
    [[vk::location(3)]] float2 velocity : TEXCOORD3;
                         float4 sv_position : SV_POSITION;
};

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    // Debug view: visualise the second UV set (TEXCOORD1, the per-vertex velocity stream)
    // instead of the normal water shading.
    if (surface_params.uv2_view != 0u) {
        // x holds the vertical velocity delta; log-scaled so small wave values and large
        // kick values are both visible. Positive = green, negative = red, zero = dark grey.
        float value = input.velocity.x;
        float intensity = saturate(log2(1.0 + abs(value)) / 8.0);
        float3 color = value >= 0.0
            ? float3(0.0, intensity, intensity * 0.35)
            : float3(intensity, intensity * 0.2, 0.0);
        return float4(color + 0.02, 1.0);
    }

    uint count = max(surface_params.count, 2u);
    float span = max(surface_params.x_max - surface_params.x_min, 0.00001);

    // Interpolate the water surface height at this pixel's X
    float t = saturate((input.world_x - surface_params.x_min) / span) * float(count - 1u);
    uint index = min((uint)floor(t), count - 2u);
    float frac_part = saturate(t - float(index));
    float surface_y = lerp(surface_heights[index], surface_heights[index + 1u], frac_part);

    // Distance below the surface, normalised by max_depth. Inverted so that larger (whiter)
    // values are higher up: 1 at the surface, 0 at max_depth.
    float depth = saturate((surface_y - input.world_y) / max(surface_params.max_depth, 0.00001));
    float height = 1.0 - depth;

    // World-space vertical gradient
    float gradient_span = max(water_look.params.y - water_look.params.z, 0.00001);
    float gradient_t = saturate((input.world_y - water_look.params.z) / gradient_span);
    float3 base = lerp(water_look.bottom_color.rgb, water_look.top_color.rgb, gradient_t);

    // Light overlay gradient in the areas above the threshold (near the surface)
    float3 overlay_gradient = lerp(water_look.highlight_bottom.rgb, water_look.highlight_top.rgb, gradient_t);
    float overlay = smoothstep(water_look.params.x, 1.0, height);

    return float4(lerp(base, overlay_gradient, overlay), 1.0);
}
