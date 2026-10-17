// Fullscreen UV quad vertex shader (pill_assets regenerates the .wgsl).
// The quad is generated from SV_VertexID in clip space, so it needs no vertex buffer and
// always covers the whole window, whatever its size.

struct VertexOutput {
    float4 sv_position            : SV_Position;
    [[vk::location(0)]] float2 uv : TEXCOORD0;
};

[shader("vertex")]
VertexOutput vs_main(uint vertex_id : SV_VertexID) {
    float2 corners[6] = {
        float2(-1.0, -1.0),
        float2( 1.0, -1.0),
        float2( 1.0,  1.0),
        float2(-1.0, -1.0),
        float2( 1.0,  1.0),
        float2(-1.0,  1.0)
    };

    float2 corner = corners[vertex_id];
    VertexOutput output;
    output.sv_position = float4(corner, 0.0, 1.0);
    // Clip space to UV: V grows downwards, like texture coordinates.
    output.uv = float2(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return output;
}
