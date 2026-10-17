// Water mesh fragment shader — the "water_mesh_shader" material (pill_assets generates the .wgsl).
// Grayscale: the value is the vertical distance from the pixel to the water surface (the top
// edge of the mesh), normalised by `max_depth`. Surface = black, bottom = white.

struct SurfaceParams {
    uint  count;     // number of surface samples
    float x_min;     // world X of the first sample
    float x_max;     // world X of the last sample
    float max_depth; // depth mapped to white
};
[[vk::binding(2, 0)]] ConstantBuffer<SurfaceParams> surface_params;
// Water surface height (world Y) per sample, uniformly spaced between x_min and x_max.
[[vk::binding(3, 0)]] StructuredBuffer<float> surface_heights;

struct FragmentInput {
    [[vk::location(0)]] float2 uv      : TEXCOORD0;
    [[vk::location(1)]] float  world_x : TEXCOORD1;
    [[vk::location(2)]] float  world_y : TEXCOORD2;
                         float4 sv_position : SV_POSITION;
};

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
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
