use std::{error::Error, rc::Rc};

pub trait Renderer {
        fn draw(&mut self, imgui_draw_data: &imgui::DrawData) -> Result<(), Box<dyn Error>>;
}
