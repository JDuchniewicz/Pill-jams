// Post-process fullscreen triangle vertex shader (pill_assets regenerates the .wgsl).
// Same trick as the engine tonemap pass: three vertices covering the whole screen, generated
// from SV_VertexID, so no vertex buffer is needed.

struct VertexOutput {
    float4 sv_position         : SV_Position;
    float2 texture_coordinates : TEXCOORD0;
};

[shader("vertex")]
VertexOutput vs_main(uint vertex_id : SV_VertexID) {
    float2 positions[3] = {
        float2(-1.0, -3.0),
        float2( 3.0,  1.0),
        float2(-1.0,  1.0)
    };

    VertexOutput output;
    output.sv_position         = float4(positions[vertex_id], 0.0, 1.0);
    output.texture_coordinates = float2(
        positions[vertex_id].x * 0.5 + 0.5,
        -positions[vertex_id].y * 0.5 + 0.5
    );
    return output;
}
