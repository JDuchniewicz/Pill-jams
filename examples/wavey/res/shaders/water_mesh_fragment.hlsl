// Water mesh fragment shader — the "water_mesh_shader" material (pill_assets generates the .wgsl).
// Grayscale: the value is the vertical distance from the pixel to the water surface (the top
// edge of the mesh), normalised by `max_depth`. Surface = black, bottom = white.
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

    // Distance below the surface, 0 at the surface and 1 at max_depth
    float depth = saturate((surface_y - input.world_y) / max(surface_params.max_depth, 0.00001));
    return float4(depth, depth, depth, 1.0);
}
