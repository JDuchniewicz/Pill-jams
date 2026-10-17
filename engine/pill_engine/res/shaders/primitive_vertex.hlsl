// Primitive line vertex shader (pill_assets regenerates the .wgsl).
// Camera layout mirrors PrimitiveCameraUniform in pass_primitives.rs.

struct Camera {
    column_major float4x4 view_projection;
    float2                viewport;
    float2                _padding;
};
[[vk::binding(0, 0)]] ConstantBuffer<Camera> camera;

struct VertexInput {
    [[vk::location(0)]] float3 position;
    [[vk::location(1)]] float4 color;
};

struct VertexOutput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float4 color : TEXCOORD0;
};

[shader("vertex")]
VertexOutput vs_main(VertexInput input) {
    VertexOutput output;
    output.sv_position = mul(camera.view_projection, float4(input.position, 1.0));
    output.color = input.color;
    return output;
}
