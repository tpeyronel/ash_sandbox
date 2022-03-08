use bevy_ecs::prelude::Component;

use crate::asset_manager::CubemapId;

#[derive(Debug, Component)]
pub struct Skybox(pub CubemapId);
