use bevy_ecs::{prelude::*, system::Command};

use crate::{
        asset_manager::{AssetManager, ModelId},
        components::{Children, Parent},
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
                let child = Self::create_model_instance_recursive(world, self.entity, self.model);

                let mut entity_mut = world.entity_mut(self.entity);

                if let Some(mut children) = entity_mut.get_mut::<Children>() {
                        children.0.push(child);
                } else {
                        entity_mut.insert(Children(vec![child]));
                }
        }
}

impl CreateModelInstance {
        fn create_model_instance_recursive(world: &mut World, parent: Entity, model_id: ModelId) -> Entity {
                let entity_id = world.spawn().id();

                let model = world.get_resource::<AssetManager>().unwrap().model(model_id);
                let model_transform = model.base_transform;
                let model_children = model.children.clone();

                let children: Vec<Entity> =
                        model_children
                                .iter()
                                .map(|&child_model_id| {
                                        Self::create_model_instance_recursive(world, entity_id, child_model_id)
                                })
                                .collect();

                world.entity_mut(entity_id)
                        .insert(Parent(parent))
                        .insert(Children(children))
                        .insert(model_transform)
                        .insert(ModelInstance { model: model_id });

                entity_id
        }
}
