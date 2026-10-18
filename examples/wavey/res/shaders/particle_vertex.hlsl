// Particle vertex shader (pill_assets regenerates the .wgsl).
// Expands every particle instance into a camera-facing textured quad.

struct Camera {
    column_major float4x4 view_projection;
    float4                camera_right; // xyz = world-space right, w unused
    float4                camera_up;    // xyz = world-space up, w unused
};
[[vk::binding(0, 0)]] ConstantBuffer<Camera> camera;

struct VertexInput {
    [[vk::location(0)]] float3 center; // per-instance, world space
    [[vk::location(1)]] float  size;   // per-instance, quad half-size in world units
    [[vk::location(2)]] float4 color;  // per-instance, linear RGBA tint
};

struct VertexOutput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float2 uv    : TEXCOORD0;
    [[vk::location(1)]] float4 color : TEXCOORD1;
};

[shader("vertex")]
VertexOutput vs_main(VertexInput input, uint vertex_id : SV_VertexID) {
    float2 corners[6] = {
        float2(0.0, 0.0),
        float2(1.0, 0.0),
        float2(1.0, 1.0),
        float2(0.0, 0.0),
        float2(1.0, 1.0),
        float2(0.0, 1.0)
    };

    float2 corner = corners[vertex_id];
    // The quad always lies in the camera plane, so particles stay billboarded.
    float2 centered = corner * 2.0 - 1.0;
    float2 offset = centered * input.size;
    float3 world_position =
        input.center + camera.camera_right.xyz * offset.x + camera.camera_up.xyz * offset.y;

    VertexOutput output;
    output.sv_position = mul(camera.view_projection, float4(world_position, 1.0));
    // Textures store V top-down, so flip the corner's vertical axis into UV space.
    output.uv = float2(corner.x, 1.0 - corner.y);
    output.color = input.color;
    return output;
}
