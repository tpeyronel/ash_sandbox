use crate::asset_manager::{MagFilter, MinFilter};

pub const DESIRED_SWAPCHAIN_IMG_COUNT: u32 = 3;
pub const MAX_CONCURRENT_FRAMES: usize = 2;
pub const ENABLE_ANISOTROPY: bool = true;
pub const LOD_CLAMP_NONE: f32 = ash::vk::LOD_CLAMP_NONE;
pub const FONT_SIZE: f32 = 13.0;
pub const PIXELS_PER_UNIT: f32 = 700.0;
pub const PLAYER_MOVEMENT_SPEED: f32 = 2.5;
pub const ROTATION_PER_SECOND: f32 = std::f32::consts::TAU / 4.0;
pub const MAX_OBJECT_MATRICES: usize = 16384;
pub const DEFAULT_SHININESS: f32 = 32.0;
pub const DEFAULT_AMBIENT_STRENGTH: f32 = 0.01;
pub const DEFAULT_SPECULAR_STRENGTH: f32 = 0.5;
pub const DEFAULT_DIFFUSE_STRENGTH: f32 = 1.0;
pub const DEFAULT_MAG_FILTER: MagFilter = MagFilter::Linear;
pub const DEFAULT_MIN_FILTER: MinFilter = MinFilter::LinearMipmapLinear;
