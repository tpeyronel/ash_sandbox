use std::error::Error;

use crate::{camera::Camera};

pub trait Renderer {
        fn draw(&mut self, cam: &Camera, imgui_draw_data: &imgui::DrawData) -> Result<(), Box<dyn Error>>;
        fn on_window_resize(&mut self, width: u32, height: u32);
        //fn set_camera_pos(&mut self, pos: &Vec3);
}
