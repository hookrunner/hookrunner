use bevy::{
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

/// q3map2 irradiance is already shaded and shadowed. Applying Bevy's PBR
/// lights/exposure again would change the authored lighting a second time.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(BakedMaterialKey)]
pub struct BakedMaterial {
    #[uniform(0)]
    pub settings: Vec4, // irradiance scale, glow scale, alpha cutoff, opaque output
    #[texture(1)]
    #[sampler(2)]
    pub diffuse: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    pub lightmap: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    pub glow: Handle<Image>,
    pub alpha: AlphaMode,
    pub double_sided: bool,
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct BakedMaterialKey {
    double_sided: bool,
    glow: bool,
    masked: bool,
    opaque: bool,
}

impl From<&BakedMaterial> for BakedMaterialKey {
    fn from(material: &BakedMaterial) -> Self {
        Self {
            double_sided: material.double_sided,
            glow: material.settings.y != 0.0,
            masked: material.settings.z > 0.0,
            opaque: material.settings.w > 0.0,
        }
    }
}

impl Material for BakedMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/baked_map.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = (!key.bind_group_data.double_sided).then_some(Face::Back);
        let fragment = descriptor.fragment.as_mut().expect("map fragment shader");
        for (enabled, name) in [
            (key.bind_group_data.glow, "MAP_GLOW"),
            (key.bind_group_data.masked, "MAP_MASKED"),
            (key.bind_group_data.opaque, "MAP_OPAQUE"),
        ] {
            if enabled {
                fragment.shader_defs.push(name.into());
            }
        }
        Ok(())
    }
}
