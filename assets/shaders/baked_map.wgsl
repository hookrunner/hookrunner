#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> settings: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var glow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var glow_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let diffuse = textureSample(diffuse_map, diffuse_sampler, in.uv);
#ifdef MAP_MASKED
    if diffuse.a < settings.z { discard; }
#endif
    // All color textures are uploaded as sRGB; sampling decodes them to linear.
    // Level 0 prevents light bleeding between islands in the baked atlas.
    let irradiance = textureSampleLevel(lightmap, lightmap_sampler, in.uv_b, 0.0).rgb;
#ifdef MAP_GLOW
    let glow = textureSample(glow_map, glow_sampler, in.uv).rgb;
#else
    let glow = vec3<f32>(0.0);
#endif
#ifdef MAP_OPAQUE
    let alpha = 1.0;
#else
    let alpha = diffuse.a * in.color.a;
#endif
    return vec4<f32>(diffuse.rgb * irradiance * in.color.rgb * settings.x + glow, alpha);
}
