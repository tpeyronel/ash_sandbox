use bevy_ecs::prelude::World;
use slotmap::{SecondaryMap, SlotMap};
use specs::Entity;

use crate::{
        asset_manager::{AssetManager, MeshId, ModelId},
        hashmap::HashMap,
        logic_thread::{ModelComponent, ProjectionCameraComponent, TransformComponent},
        my_glm::*,
        AnyResult,
};

slotmap::new_key_type! { pub struct ModelInstanceId; }

#[derive(Debug, Clone)]
pub struct ModelInstance {
        pub model_id: ModelId,
        pub mesh_instances: Vec<MeshInstanceId>,
        pub children: Vec<ModelInstanceId>,
        pub transform: TransformComponent,
}

slotmap::new_key_type! { pub struct MeshInstanceId; }

#[derive(Debug, Clone)]
pub struct MeshInstance {
        pub mesh_id: MeshId,
        pub transform: TransformComponent,
}

#[derive(Debug, Clone, Copy)]
pub struct LightPos(pub Vec3);

#[derive(Debug, Clone, Copy)]
pub struct LightColor(pub Vec3);

pub trait Renderer {
        // fn draw(&mut self, cam: &mut Camera, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
        fn draw(
                &mut self,
                mesh_instances: &SlotMap<MeshInstanceId, MeshInstance>,
                model_instances: &SlotMap<ModelInstanceId, ModelInstance>,
                model_instances_index: &HashMap<Entity, ModelInstanceId>,
                transform_manager: &TransformManager,
                render_state: &RenderState,
                player_orien: &UnitQuat,
                imgui_draw_data: &imgui::DrawData,
        ) -> AnyResult<()>;
        fn draw_world(&mut self, world: &mut World, imgui_draw_data: &imgui::DrawData) -> AnyResult<()>;
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
        pub asset_manager: Option<AssetManager>,
        pub camera_pos: Vec3,
        pub proj_camera: ProjectionCameraComponent,
        pub model_instances: HashMap<Entity, (TransformComponent, ModelComponent)>,
        pub lights: HashMap<Entity, (LightPos, LightColor)>,
}

impl RenderState {
        pub fn new() -> Self {
                Self {
                        asset_manager: None,
                        camera_pos: Default::default(),
                        proj_camera: Default::default(),
                        model_instances: HashMap::new(),
                        lights: HashMap::new(),
                }
        }

        pub fn interpolate(old: &Self, new: &Self, t: f32) -> Self {
                let camera_pos = Vec3::lerp(&old.camera_pos, &new.camera_pos, t);

                let mut model_instances = new.model_instances.clone();
                for (model_instance_id, (new_instance_transform, _)) in &mut model_instances {
                        if let Some((old_instance_transform, _)) = old.model_instances.get(model_instance_id) {
                                new_instance_transform.pos =
                                        Vec3::lerp(&old_instance_transform.pos, &new_instance_transform.pos, t);
                                new_instance_transform.orien = UnitQuat::nlerp(
                                        &old_instance_transform.orien,
                                        &new_instance_transform.orien,
                                        t,
                                );
                                new_instance_transform.scale =
                                        Vec3::lerp(&old_instance_transform.scale, &new_instance_transform.scale, t);
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
                        asset_manager: new.asset_manager.clone(),
                        camera_pos,
                        proj_camera: new.proj_camera,
                        model_instances,
                        lights,
                }
        }
}

slotmap::new_key_type! {
        pub struct TransformId;
}

pub struct TransformManager {
        transforms: Vec<SecondaryMap<MeshInstanceId, Mat4>>,
        update_index: usize,
}

impl TransformManager {
        pub fn new(update_history: usize) -> Self {
                Self {
                        transforms: vec![SecondaryMap::new(); update_history],
                        update_index: update_history - 1,
                }
        }

        pub fn on_update(&mut self) {
                self.update_index = (self.update_index + 1) % self.transforms.len();
                self.transforms[self.update_index].clear();
        }

        pub fn set_transform(&mut self, mesh_instance_id: MeshInstanceId, transform: &Mat4) {
                self.transforms[self.update_index].insert(mesh_instance_id, *transform);
        }

        pub fn get_transform(&self, mesh_instance_id: MeshInstanceId) -> &Mat4 {
                &self.transforms[self.update_index][mesh_instance_id]
        }

        pub fn iter(&self) -> impl Iterator<Item = (MeshInstanceId, &Mat4)> + '_ {
                let split_index = (self.update_index + 1) % self.transforms.len();
                let (new, old) = self.transforms.split_at(split_index);

                old.iter().chain(new.iter()).flatten()
        }
}

// pub struct ModelTransformManager {
//         transforms: Vec<Mat4>,
//         indices: HashMap<ModelInstanceId, usize>,
// }

// impl ModelTransformManager {
//         pub fn new() -> Self {
//                 Self {
//                         transforms: Vec::new(),
//                         indices: HashMap::new(),
//                 }
//         }

//         pub fn update(&mut self) {
//                 self.transforms.clear();
//                 self.indices.clear();
//         }

//         pub fn set_transform(&mut self, id: &ModelInstanceId, transform: Mat4) {
//                 match self.indices.get(id) {
//                         Some(&i) => {
//                                 self.transforms[i] = transform;
//                         },
//                         None => {
//                                 self.indices.insert(*id, self.transforms.len());
//                                 self.transforms.push(transform);
//                         },
//                 }
//         }

//         pub fn for_each_transform(&self, mut f: impl FnMut(usize, &Mat4)) {
//                 for (_, &i) in &self.indices {
//                         f(i, &self.transforms[i]);
//                 }
//         }

//         // pub fn iter(&self) -> impl Iterator<Item = (&ModelInstanceId, &TransformComponent)> + '_ {
//         //         let split_index = (self.update_index + 1) % self.update_history;
//         //         let (new, old) = self.transforms_history.split_at(split_index);

//         //         old.iter().chain(new.iter()).flatten()
//         // }
// }
