// Water mesh vertex shader — the "water_mesh_shader" material (pill_assets generates the .wgsl).
// Same per-draw convention as the PBR and cartoon passes: model = T * (Rx*Ry*Rz) * S on the GPU.

struct Camera {
    column_major float4x4 view_projection;
};
[[vk::binding(0, 0)]] ConstantBuffer<Camera> camera;

struct PerDraw {
    float4 position; // xyz
    float4 rotation; // xyz, radians
    float4 scale;    // xyz
};
[[vk::binding(1, 0)]] StructuredBuffer<PerDraw> per_draw_array;

struct VertexInput {
    [[vk::location(0)]] float3 position;
    [[vk::location(4)]] float2 uv;
};

struct VertexOutput {
    [[vk::location(0)]] float2 uv        : TEXCOORD0;
    [[vk::location(1)]] float  world_x   : TEXCOORD1;
    [[vk::location(2)]] float  world_y   : TEXCOORD2;
                         float4 sv_position : SV_POSITION;
};

float3x3 rot_x(float angle) { float cosine = cos(angle), sine = sin(angle); return float3x3(1, 0, 0,  0, cosine, -sine,  0, sine, cosine); }
float3x3 rot_y(float angle) { float cosine = cos(angle), sine = sin(angle); return float3x3(cosine, 0, sine,  0, 1, 0,  -sine, 0, cosine); }
float3x3 rot_z(float angle) { float cosine = cos(angle), sine = sin(angle); return float3x3(cosine, -sine, 0,  sine, cosine, 0,  0, 0, 1); }

[shader("vertex")]
VertexOutput vs_main(VertexInput input, uint instance_id : SV_InstanceID) {
    PerDraw per_draw = per_draw_array[instance_id];

    float3x3 rotation = mul(rot_x(per_draw.rotation.x), mul(rot_y(per_draw.rotation.y), rot_z(per_draw.rotation.z)));
    float3 world_position = per_draw.position.xyz + mul(rotation, input.position * per_draw.scale.xyz);

    VertexOutput output;
    output.sv_position = mul(camera.view_projection, float4(world_position, 1.0));
    output.uv = input.uv;
    output.world_x = world_position.x;
    output.world_y = world_position.y;
    return output;
}
