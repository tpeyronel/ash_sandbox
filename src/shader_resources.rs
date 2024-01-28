use crate::shader_resource::ShaderResourceId;

use lazy_static::lazy_static;

lazy_static! {
        pub static ref SHADER_RESOURCE_WORLD_MATRICES: ShaderResourceId = "WORLD_MATRICES".into();
        pub static ref SHADER_RESOURCE_WORLD_LIGHTS: ShaderResourceId = "WORLD_LIGHTS".into();
        pub static ref SHADER_RESOURCE_BILLBOARD_DATA: ShaderResourceId = "BILLBOARD_DATA".into();
        pub static ref SHADER_RESOURCE_SKYBOX: ShaderResourceId = "SKYBOX".into();
        pub static ref SHADER_RESOURCE_MATERIAL_DATA: ShaderResourceId = "MATERIAL_DATA".into();
        pub static ref SHADER_RESOURCE_MATERIAL_DIFFUSE_TEXTURE: ShaderResourceId = "MATERIAL_DIFFUSE_TEXTURE".into();
        pub static ref SHADER_RESOURCE_MATERIAL_SPECULAR_TEXTURE: ShaderResourceId = "MATERIAL_SPECULAR_TEXTURE".into();
        pub static ref SHADER_RESOURCE_OBJECT_MATRICES: ShaderResourceId = "OBJECT_MATRICES".into();
}
