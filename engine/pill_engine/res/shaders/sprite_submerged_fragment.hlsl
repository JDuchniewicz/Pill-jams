// Submerged sprite fragment shader (pill_assets regenerates the .wgsl).
// Everything below the per-sprite waterline is multiplied by the submerge factor, so the part
// of a sprite that is under the water surface appears darker.

struct FragmentInput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float2 uv       : TEXCOORD0;
    [[vk::location(1)]] float4 color    : TEXCOORD1;
    [[vk::location(2)]] float2 submerge : TEXCOORD2;
    [[vk::location(3)]] float  world_y  : TEXCOORD3;
};

// Sprite texture bind group (set 1) — mirrors PassSprites' second bind group.
[[vk::binding(0, 1)]] Texture2D    sprite_texture;
[[vk::binding(1, 1)]] SamplerState sprite_sampler;

[shader("fragment")]
float4 fs_main(FragmentInput input) : SV_Target {
    float4 texel = sprite_texture.Sample(sprite_sampler, input.uv);
    float3 rgb = texel.rgb * input.color.rgb;

    // Submerged part: below the waterline, scale the color (0.5 = half brightness).
    float submerge_factor = lerp(input.submerge.y, 1.0, step(input.submerge.x, input.world_y));
    rgb *= submerge_factor;

    return float4(rgb, texel.a * input.color.a);
}
