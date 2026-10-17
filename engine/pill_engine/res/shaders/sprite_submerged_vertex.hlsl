// Submerged sprite vertex shader (pill_assets regenerates the .wgsl).
// Same quad as sprite_vertex.hlsl, but it also forwards the per-instance waterline and the
// fragment's world Y so the fragment shader can darken the part under the water surface.

struct Camera {
    column_major float4x4 view_projection;
    float4                camera_right; // xyz = world-space right, w unused
    float4                camera_up;    // xyz = world-space up, w unused
};
[[vk::binding(0, 0)]] ConstantBuffer<Camera> camera;

struct VertexInput {
    [[vk::location(0)]] float3 center;     // per-instance, world space
    [[vk::location(1)]] float2 axis_x;     // per-instance, world units (size + scale + rotation)
    [[vk::location(2)]] float2 axis_y;     // per-instance, world units (size + scale + rotation)
    [[vk::location(3)]] float4 color;      // per-instance, linear RGBA tint
    [[vk::location(4)]] float2 uv_flip;    // per-instance, 1 = mirrored on that UV axis
    [[vk::location(5)]] float  depth_test; // per-instance, 0 = always drawn on top
    [[vk::location(6)]] float2 submerge;   // per-instance: x = waterline world Y, y = factor below
};

struct VertexOutput {
    float4 sv_position       : SV_Position;
    [[vk::location(0)]] float2 uv       : TEXCOORD0;
    [[vk::location(1)]] float4 color    : TEXCOORD1;
    [[vk::location(2)]] float2 submerge : TEXCOORD2;
    [[vk::location(3)]] float  world_y  : TEXCOORD3;
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
    // The quad always lies in the camera plane, so sprites stay screen-aligned for any camera.
    // The axes already include the transform rotation, projected onto that plane.
    float2 centered = corner * 2.0 - 1.0;
    float2 offset = centered.x * input.axis_x + centered.y * input.axis_y;
    float3 world_position =
        input.center + camera.camera_right.xyz * offset.x + camera.camera_up.xyz * offset.y;
    float4 clip_position = mul(camera.view_projection, float4(world_position, 1.0));
    if (input.depth_test < 0.5) {
        // Nearest depth: passes every LessEqual test, so the sprite is never occluded.
        clip_position.z = 0.0;
    }

    // Textures store V top-down, so flip the corner's vertical axis into UV space.
    float2 uv = float2(corner.x, 1.0 - corner.y);
    uv = lerp(uv, 1.0 - uv, input.uv_flip);

    VertexOutput output;
    output.sv_position = clip_position;
    output.uv = uv;
    output.color = input.color;
    output.submerge = input.submerge;
    output.world_y = world_position.y;
    return output;
}
