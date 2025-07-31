use bevy_ecs::prelude::World;
use bytemuck::NoUninit;
use shader_resource_derive::ShaderStruct;

use crate::{my_glm::Vec2, AnyResult};

pub trait Renderer {
        fn draw_world(&mut self, world: &mut World, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn on_window_resize(&mut self, width: u32, height: u32);
        fn destroy(&mut self) -> AnyResult<()>;
}

#[repr(C)]
#[derive(Clone, Copy, NoUninit, ShaderStruct)]
pub struct PrefilterParams {
        pub roughness_and_env_map_size: Vec2,
}
