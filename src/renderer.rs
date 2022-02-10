use specs::Entity;

use crate::{asset_manager::ModelId, hashmap::HashMap, logic_thread::ProjectionCameraComponent, my_glm::*, AnyResult};

#[derive(Debug, Clone)]
pub struct ModelInstance {
        pub model_id: ModelId,
        pub pos: Vec3,
        pub orien: UnitQuat,
        pub scale: Vec3,
}

#[derive(Clone, Copy, Debug, Hash, Eq, Ord, PartialEq, PartialOrd)]
pub struct ModelInstanceId(pub Entity);

#[derive(Debug, Clone, Copy)]
pub struct LightPos(pub Vec3);

#[derive(Debug, Clone, Copy)]
pub struct LightColor(pub Vec3);

pub trait Renderer {
        // fn draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn draw(&mut self, render_state: &RenderState, player_orien: &UnitQuat, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn on_window_resize(&mut self, width: u32, height: u32);
        fn destroy(&mut self) -> AnyResult<()>;

        /*
        fn begin_frame(&mut self);
        fn draw_model_instance(&mut self, model_instance: ModelInstanceID);
        fn end_frame_and_draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData);
        */
}

#[derive(Debug, Clone)]
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

        pub fn interpolate(old: &Self, new: &Self, t: f32) -> Self {
                let camera_pos = Vec3::lerp(&old.camera_pos, &new.camera_pos, t);

                let mut model_instances = new.model_instances.clone();
                for (model_instance_id, new_instance) in &mut model_instances {
                        if let Some(old_instance) = old.model_instances.get(model_instance_id) {
                                new_instance.pos = Vec3::lerp(&old_instance.pos, &new_instance.pos, t);
                                new_instance.orien = UnitQuat::nlerp(&old_instance.orien, &new_instance.orien, t);
                                new_instance.scale = Vec3::lerp(&old_instance.scale, &new_instance.scale, t);
                        }
                }

                let mut lights = new.lights.clone();
                for (light_id, new_light) in &mut lights {
                        if let Some(old_light) = old.lights.get(light_id) {
                                new_light.0 .0 = Vec3::lerp(&old_light.0 .0, &new_light.0 .0, t);
                                new_light.1 .0 = Vec3::lerp(&old_light.1 .0, &new_light.1 .0, t);
                        }
                }

                Self {
                        camera_pos,
                        proj_camera: new.proj_camera,
                        model_instances,
                        lights,
                }
        }
}
