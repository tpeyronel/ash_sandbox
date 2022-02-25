use bevy_ecs::prelude::World;

use crate::AnyResult;

pub trait Renderer {
        fn draw_world(&mut self, world: &mut World, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn on_window_resize(&mut self, width: u32, height: u32);
        fn destroy(&mut self) -> AnyResult<()>;
}
