// Primitive fragment shader (pill_assets regenerates the .wgsl).

struct FragmentInput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float4 color : TEXCOORD0;
};

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    return input.color;
}
