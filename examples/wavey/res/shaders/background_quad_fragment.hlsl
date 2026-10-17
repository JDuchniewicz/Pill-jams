// Background quad fragment shader: 2D sky above the water line and water below it.
// Every parameter comes from SkyParams (mirrored by background_pass.rs, edited in the egui UI).

struct SkyParams {
    float4 sky_top;       // rgb = sky color at the top of the window
    float4 sky_horizon;   // rgb = sky color at the water line
    float4 water_horizon; // rgb = water color just below the water line
    float4 water_bottom;  // rgb = water color at the bottom of the window
    float4 sun_color;     // rgb = glow color, w = intensity
    float4 sun;           // xy = center (0..1 UV), z = radius, w = edge softness
    float4 shape;         // x = water line V (0..1), y = softness, z = wave amplitude, w = wave frequency
    float4 extra;         // x = time (seconds), y = wave speed (rad/s)
};
[[vk::binding(0, 0)]] ConstantBuffer<SkyParams> params;

struct FragmentInput {
    float4 sv_position            : SV_Position;
    [[vk::location(0)]] float2 uv : TEXCOORD0;
};

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    float v = input.uv.y; // 0 = top of the window, 1 = bottom

    // Wavy water line: a sine ripple scrolls horizontally over time
    float ripple = sin(input.uv.x * params.shape.w + params.extra.x * params.extra.y) * params.shape.z;
    float water_line = params.shape.x + ripple;
    float line_softness = max(params.shape.y, 0.0005);

    // Sky: gradient from the top edge down to the water line
    float sky_t = saturate(v / max(water_line, 0.0005));
    float3 sky = lerp(params.sky_top.rgb, params.sky_horizon.rgb, sky_t);

    // Water: gradient from the water line down to the bottom edge
    float water_t = saturate((v - water_line) / max(1.0 - water_line, 0.0005));
    float3 water = lerp(params.water_horizon.rgb, params.water_bottom.rgb, water_t);

    // Soft transition between sky and water at the water line
    float blend = smoothstep(water_line - line_softness, water_line + line_softness, v);
    float3 color = lerp(sky, water, blend);

    // Sun / moon glow
    float2 to_sun = input.uv - params.sun.xy;
    float sun_distance = length(to_sun) / max(params.sun.z, 0.0005);
    float glow = params.sun_color.w *
        saturate(1.0 - smoothstep(1.0 - params.sun.w, 1.0, sun_distance));
    color += params.sun_color.rgb * glow;

    return float4(color, 1.0);
}
