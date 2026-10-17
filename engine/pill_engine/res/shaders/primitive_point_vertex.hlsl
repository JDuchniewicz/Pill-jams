// Primitive point vertex shader (pill_assets regenerates the .wgsl).
// Expands each point instance into a screen-space quad of camera.point_size pixels.

struct Camera {
    column_major float4x4 view_projection;
    float2                viewport;
    float                 point_size;
    float                 _padding;
};
[[vk::binding(0, 0)]] ConstantBuffer<Camera> camera;

struct VertexInput {
    [[vk::location(0)]] float3 position; // per-instance
    [[vk::location(1)]] float4 color;    // per-instance
};

struct VertexOutput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float4 color : TEXCOORD0;
};

[shader("vertex")]
VertexOutput vs_main(VertexInput input, uint vertex_id : SV_VertexID) {
    float2 corners[6] = {
        float2(-1.0, -1.0),
        float2( 1.0, -1.0),
        float2( 1.0,  1.0),
        float2(-1.0, -1.0),
        float2( 1.0,  1.0),
        float2(-1.0,  1.0)
    };

    VertexOutput output;
    float4 clip_position = mul(camera.view_projection, float4(input.position, 1.0));
    float2 pixel_offset = corners[vertex_id] * camera.point_size / camera.viewport;
    clip_position.x += pixel_offset.x * clip_position.w;
    clip_position.y += pixel_offset.y * clip_position.w;
    output.sv_position = clip_position;
    output.color = input.color;
    return output;
}
