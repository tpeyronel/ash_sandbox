use bevy_ecs::prelude::*;

use crate::{
        asset_manager::{AssetManager, ModelId},
        my_glm::Mat4,
};

#[derive(Component, Debug, Clone)]
pub struct ModelInstance {
        pub model: ModelId,
        pub transform: TransformId,
        pub children: Vec<ModelInstance>,
}

pub struct ModelInstanceManager;

impl ModelInstanceManager {
        pub fn new() -> Self {
                Self
        }

        pub fn create_model_instance(
                &self,
                asset_manager: &AssetManager,
                transform_manager: &mut TransformManager,
                model_id: ModelId,
        ) -> ModelInstance {
                let model = &asset_manager.models()[model_id];

                let transform = transform_manager.insert_transform(&Mat4::identity());

                let children = model
                        .children
                        .iter()
                        .map(|&mid| self.create_model_instance(asset_manager, transform_manager, mid))
                        .collect();

                ModelInstance {
                        model: model_id,
                        transform,
                        children,
                }
        }
}

slotmap::new_key_type! {
        pub struct TransformId;
}

pub struct TransformManager {
        transforms: slotmap::SlotMap<TransformId, Mat4>,
        transform_updates: Vec<slotmap::SecondaryMap<TransformId, Mat4>>,
        update_index: usize,
}

impl TransformManager {
        pub fn new(update_history: usize) -> Self {
                Self {
                        transforms: slotmap::SlotMap::with_key(),
                        transform_updates: vec![slotmap::SecondaryMap::new(); update_history],
                        update_index: update_history - 1,
                }
        }

        pub fn on_update(&mut self) {
                self.update_index = (self.update_index + 1) % self.transform_updates.len();
                self.transform_updates[self.update_index].clear();
        }

        pub fn insert_transform(&mut self, transform: &Mat4) -> TransformId {
                self.transforms.insert(*transform)
        }

        pub fn set_transform(&mut self, transform_id: TransformId, transform: &Mat4) {
                self.transforms[transform_id] = *transform;
                self.transform_updates[self.update_index].insert(transform_id, *transform);
        }

        pub fn get_transform(&self, transform_id: TransformId) -> &Mat4 {
                &self.transform_updates[self.update_index][transform_id]
        }

        pub fn iter_transform_updates(&self) -> impl Iterator<Item = (TransformId, &Mat4)> + '_ {
                let split_index = (self.update_index + 1) % self.transform_updates.len();
                let (new, old) = self.transform_updates.split_at(split_index);

                old.iter().chain(new.iter()).flatten()
        }
}
