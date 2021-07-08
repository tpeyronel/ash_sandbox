use std::{cell::RefCell, error::Error, rc::Rc};

use crate::{asset_manager::ModelID, camera::Camera, my_glm::*};

struct ModelInstance {
        model: ModelID,
        transform: Mat4,
}

struct ModelInstanceID(usize);

pub trait Renderer {
       /*  fn begin_frame(&mut self);
        fn draw_model_instance(&mut self, model_instance: ModelInstanceID);
        fn end_frame_and_draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData); */
        fn draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData) -> Result<(), Box<dyn Error>>;
        fn on_window_resize(&mut self, width: u32, height: u32);
        //fn set_camera_pos(&mut self, pos: &Vec3);
}

/* pub trait Renderer {
        fn new(cam: Arc<Mutex<Camera>>);
} */

pub struct RenderState {
        model_instances: Vec<ModelInstance>,
}