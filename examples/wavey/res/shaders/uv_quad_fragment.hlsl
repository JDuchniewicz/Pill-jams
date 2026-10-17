// Fullscreen UV rainbow fragment shader (pill_assets regenerates the .wgsl).
//
// Classic UV visualization: U in red, V in green.
// For a full hue rainbow across U instead, replace the return with:
//   float3 hsv = float3(input.uv.x, 1.0, 1.0);
//   float3 rgb = saturate(abs(frac(hsv.x + float3(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0);
//   return float4(rgb * input.uv.y, 1.0);

struct FragmentInput {
    float4 sv_position            : SV_Position;
    [[vk::location(0)]] float2 uv : TEXCOORD0;
};

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    return float4(input.uv.x, input.uv.y, 0.0, 1.0);
}
