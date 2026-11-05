// Slang compiles this source to native Metal source and validated SPIR-V.
// Keep these layouts synchronized with native/renderer.c static assertions.
struct FrameData {
    float4x4 mvp;
    float4x4 model;
};
struct MaterialData {
    float4 color;
    float4 light; // xyz points toward the light; w is ambient intensity.
};
[[vk::binding(0, 1)]]
ConstantBuffer<FrameData> frame : register(b0, space1);
[[vk::binding(0, 3)]]
ConstantBuffer<MaterialData> material : register(b0, space3);

struct VertexInput {
    float3 position : TEXCOORD0;
    float3 normal : TEXCOORD1;
};
struct Raster {
    float4 position : SV_Position;
    float3 normal : TEXCOORD0;
};

[shader("vertex")]
Raster vertex_main(VertexInput input) {
    Raster output;
    output.position = mul(frame.mvp, float4(input.position, 1.0));
    output.normal = mul(frame.model, float4(input.normal, 0.0)).xyz;
    return output;
}

[shader("fragment")]
float4 fragment_main(Raster input) : SV_Target0 {
    float diffuse = max(dot(normalize(input.normal), normalize(material.light.xyz)), 0.0);
    float intensity = material.light.w + (1.0 - material.light.w) * diffuse;
    return float4(material.color.rgb * intensity, material.color.a);
}
