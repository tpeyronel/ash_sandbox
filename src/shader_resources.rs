use crate::shader_resource::ShaderResourceId;

use lazy_static::lazy_static;

lazy_static! {
        pub static ref SHADER_RESOURCE_INPUT_FRAMEBUFFER: ShaderResourceId = "INPUT_FRAMEBUFFER".into();
        pub static ref SHADER_RESOURCE_SHADER_SETTINGS: ShaderResourceId = "SHADER_SETTINGS".into();
        pub static ref SHADER_RESOURCE_WORLD_MATRICES: ShaderResourceId = "WORLD_MATRICES".into();
        pub static ref SHADER_RESOURCE_WORLD_LIGHTS: ShaderResourceId = "WORLD_LIGHTS".into();
        pub static ref SHADER_RESOURCE_BILLBOARD_DATA: ShaderResourceId = "BILLBOARD_DATA".into();
        pub static ref SHADER_RESOURCE_SKYBOX: ShaderResourceId = "SKYBOX".into();
        pub static ref SHADER_RESOURCE_MATERIAL_DATA: ShaderResourceId = "MATERIAL_DATA".into();
        pub static ref SHADER_RESOURCE_MATERIAL_BASE_COLOR_TEXTURE: ShaderResourceId = "MATERIAL_BASE_COLOR_TEXTURE".into();
        pub static ref SHADER_RESOURCE_MATERIAL_METALLIC_ROUGHNESS_TEXTURE: ShaderResourceId = "MATERIAL_METALLIC_ROUGHNESS_TEXTURE".into();
        pub static ref SHADER_RESOURCE_MATERIAL_DIFFUSE_TEXTURE: ShaderResourceId = "MATERIAL_DIFFUSE_TEXTURE".into();
        pub static ref SHADER_RESOURCE_MATERIAL_SPECULAR_TEXTURE: ShaderResourceId = "MATERIAL_SPECULAR_TEXTURE".into();
        pub static ref SHADER_RESOURCE_MATERIAL_NORMAL_TEXTURE: ShaderResourceId = "MATERIAL_NORMAL_TEXTURE".into();
        pub static ref SHADER_RESOURCE_OBJECT_MATRICES: ShaderResourceId = "OBJECT_MATRICES".into();
        pub static ref SHADER_RESOURCE_SHADOW_MAP: ShaderResourceId = "SHADOW_MAP".into();
        pub static ref SHADER_RESOURCE_CUBE_SHADOW_MAP: ShaderResourceId = "CUBE_SHADOW_MAP".into();
}
