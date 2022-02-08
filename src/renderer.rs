use specs::Entity;

use crate::{asset_manager::ModelId, hashmap::HashMap, logic_thread::ProjectionCameraComponent, my_glm::*, AnyResult};

pub struct ModelInstance {
        pub model_id: ModelId,
        pub pos: Vec3,
        pub orien: UnitQuat,
        pub scale: Vec3,
}

#[derive(Clone, Copy, Debug, Hash, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelInstanceId(pub Entity);

pub struct LightPos(pub Vec3);
pub struct LightColor(pub Vec3);

pub trait Renderer {
        // fn draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn draw(&mut self, player_orien: &UnitQuat, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn on_window_resize(&mut self, width: u32, height: u32);
        fn destroy(&mut self) -> AnyResult<()>;

        /*
        fn begin_frame(&mut self);
        fn draw_model_instance(&mut self, model_instance: ModelInstanceID);
        fn end_frame_and_draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData);
        */
}

pub struct RenderState {
        pub camera_pos: Vec3,
        pub proj_camera: ProjectionCameraComponent,
        pub model_instances: HashMap<ModelInstanceId, ModelInstance>,
        pub lights: HashMap<Entity, (LightPos, LightColor)>,
}

impl RenderState {
        pub fn new() -> Self {
                Self {
                        camera_pos: Default::default(),
                        proj_camera: Default::default(),
                        model_instances: HashMap::new(),
                        lights: HashMap::new(),
                }
        }
}
