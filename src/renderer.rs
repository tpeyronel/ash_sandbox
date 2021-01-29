use std::error::Error;

pub trait Renderer {
        fn draw(&mut self, imgui_draw_data: &imgui::DrawData) -> Result<(), Box<dyn Error>>;
        fn on_window_resize(&mut self, width: u32, height: u32);
}
