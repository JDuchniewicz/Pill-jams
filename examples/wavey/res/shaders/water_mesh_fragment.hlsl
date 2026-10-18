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
    float4 foam_color;       // rgb = foam colour
    float4 foam;             // x = foam depth threshold, y = multiplier for the UV2 velocity value
    float4 god_rays;         // x = intensity, y = accumulated travel offset, z = angle (radians),
                             // w = base width
    float4 god_rays_color;   // rgb = ray colour, a = mesh UV V where the rays reach zero
};
[[vk::binding(4, 0)]] ConstantBuffer<WaterLook> water_look;

struct FragmentInput {
    [[vk::location(0)]] float2 uv       : TEXCOORD0;
    [[vk::location(1)]] float  world_x  : TEXCOORD1;
    [[vk::location(2)]] float  world_y  : TEXCOORD2;
    // Second UV set (water point vertical velocity); available for effects
    [[vk::location(3)]] float2 velocity : TEXCOORD3;
                         float4 sv_position : SV_POSITION;
};

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    // Debug view: visualise the second UV set (TEXCOORD1, the per-vertex point velocity stream)
    // instead of the normal water shading.
    if (surface_params.uv2_view != 0u) {
        // x holds the horizontal velocity; log-scaled so small wave values and large
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
    float3 color = lerp(base, overlay_gradient, overlay);

    // Foam: a smooth band near the surface, scaled by the second UV set (vertical velocity), so
    // stronger motion produces more foam instead of a hard on/off threshold.
    float foam_band = smoothstep(water_look.foam.x, 1.0, height);
    float foam_amount = saturate(abs(input.velocity.x) * water_look.foam.y);
    foam_amount = smoothstep(0.0, 1.0, foam_amount);
    float foam = foam_band * foam_amount;
    color = lerp(color, water_look.foam_color.rgb, foam);

    // God rays: seven irregular gradient shafts in the water mesh's UV space. Every ray has a
    // stable pseudo-random offset and width, and the accumulated offset (god_rays.y) drifts with
    // time so the shafts slowly travel through the mesh. The added value is lerped to zero towards
    // the bottom of the mesh using the first UV set (god_rays_color.a).
    float2 ray_axis = float2(cos(water_look.god_rays.z), sin(water_look.god_rays.z));
    float ray_span = max(abs(ray_axis.x) + abs(ray_axis.y), 0.0001);
    float ray_coord = dot(input.uv, ray_axis) / ray_span;
    float rays = 0.0;
    [unroll]
    for (int i = 0; i < 7; ++i) {
        float fi = float(i);
        float offset_jitter = frac(sin(fi * 12.9898 + 4.1414) * 43758.5453);
        float width_jitter = frac(sin(fi * 78.233 + 1.2345) * 43758.5453);
        float speed_jitter = frac(sin(fi * 39.346 + 7.1234) * 43758.5453);

        // One ray per slot, jittered inside it so the spacing between rays is irregular.
        float center = (fi + offset_jitter) / 7.0;
        center += water_look.god_rays.y * (0.5 + speed_jitter);
        center = frac(center);

        // Random shaft width around the slider value.
        float half_width = 0.5 * water_look.god_rays.w * (0.5 + width_jitter);
        float distance_to_ray = abs(ray_coord - center);
        distance_to_ray = min(distance_to_ray, 1.0 - distance_to_ray);
        rays += 1.0 - smoothstep(0.0, max(half_width, 0.0001), distance_to_ray);
    }
    rays = saturate(rays);

    float vertical_fade = 1.0 - saturate(input.uv.y / max(water_look.god_rays_color.a, 0.0001));
    color += water_look.god_rays_color.rgb * (rays * water_look.god_rays.x * vertical_fade);

    return float4(color, 1.0);
}
