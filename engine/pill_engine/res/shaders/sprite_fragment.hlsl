// Sprite fragment shader (pill_assets regenerates the .wgsl).
// Samples the sprite texture and multiplies it by the per-instance tint.

struct FragmentInput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float2 uv    : TEXCOORD0;
    [[vk::location(1)]] float4 color : TEXCOORD1;
};

// Sprite texture bind group (set 1) — mirrors PassSprites' second bind group.
[[vk::binding(0, 1)]] Texture2D    sprite_texture;
[[vk::binding(1, 1)]] SamplerState sprite_sampler;

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    float4 texel = sprite_texture.Sample(sprite_sampler, input.uv);
    return float4(texel.rgb * input.color.rgb, texel.a * input.color.a);
}
