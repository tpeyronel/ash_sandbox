use bevy_ecs::{prelude::*, system::Command};

use crate::{
        asset_manager::{AssetManager, ModelId},
        components::{Children, Parent, Transform},
};

#[derive(Component, Debug, Clone)]
pub struct ModelInstance {
        pub model: ModelId,
}

pub struct CreateModelInstanceFromName {
        pub entity: Entity,
        pub model_name: String,
}

impl Command for CreateModelInstanceFromName {
        fn write(self, world: &mut World) {
                let model = world
                        .get_resource::<AssetManager>()
                        .unwrap()
                        .get_model_by_name(&self.model_name);

                CreateModelInstance {
                        entity: self.entity,
                        model,
                }
                .write(world);
        }
}

pub struct CreateModelInstance {
        pub entity: Entity,
        pub model: ModelId,
}

impl Command for CreateModelInstance {
        fn write(self, world: &mut World) {
                Self::create_model_instance_recursive(world, self.entity, self.model);
        }
}

impl CreateModelInstance {
        fn create_model_instance_recursive(world: &mut World, entity: Entity, model_id: ModelId) {
                let model = &world.get_resource::<AssetManager>().unwrap().models()[model_id];
                let model_children = model.children.clone();
                let model_transform = model.base_transform;

                let children: Vec<Entity> = model_children
                        .iter()
                        .map(|&child_model_id| {
                                let entity = world.spawn().insert(Parent(entity)).id();
                                Self::create_model_instance_recursive(world, entity, child_model_id);
                                entity
                        })
                        .collect();

                let mut entity_mut = world.entity_mut(entity);

                match entity_mut.get_mut::<Transform>() {
                        Some(mut transform) => {
                                *transform = model_transform * *transform;
                        },
                        None => {
                                entity_mut.insert(model_transform);
                        },
                }

                entity_mut.insert(ModelInstance { model: model_id });

                if let Some(mut entity_children) = entity_mut.get_mut::<Children>() {
                        entity_children.0.extend(children.iter());
                } else {
                        entity_mut.insert(Children(children));
                }
        }
}
