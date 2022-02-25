use bevy_ecs::prelude::*;

use crate::my_glm::*;

pub struct Player(pub Entity);

pub struct ActiveCamera(pub Entity);

pub struct Ticktime(pub f32);

pub struct InterpScalar(pub f32);

pub struct ImguiWantCaptureMouse(pub bool);
pub struct ImguiWantCaptureKeyboard(pub bool);


#[derive(Component, Debug, Clone, Copy)]
pub struct Transform {
        pub pos: Vec3,
        pub orien: UnitQuat,
        pub scale: Vec3,
}

impl Default for Transform {
        fn default() -> Self {
                Self {
                        pos: Vec3::from_element(0.0),
                        orien: UnitQuat::identity(),
                        scale: Vec3::from_element(1.0),
                }
        }
}

impl Transform {
        pub fn interp(&self, other: &Self, t: f32) -> Self {
                Self {
                        pos: Vec3::lerp(&self.pos, &other.pos, t),
                        orien: UnitQuat::try_slerp(&self.orien, &other.orien, t, 0.0)
                                .unwrap_or_else(|| UnitQuat::nlerp(&self.orien, &other.orien, t)),
                        scale: Vec3::lerp(&self.scale, &other.scale, t),
                }
        }

        pub fn to_matrix(&self) -> Mat4 {
                Mat4::new_translation(&self.pos)
                        * UnitQuat::to_homogeneous(&self.orien)
                        * Mat4::new_nonuniform_scaling(&self.scale)
        }

        pub fn from_pos(pos: Vec3) -> Self {
                Self {
                        pos,
                        ..Default::default()
                }
        }

        pub fn from_orien(orien: UnitQuat) -> Self {
                Self {
                        orien,
                        ..Default::default()
                }
        }

        pub fn from_scale(scale: Vec3) -> Self {
                Self {
                        scale,
                        ..Default::default()
                }
        }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Velocity(pub Vec3);

#[derive(Component, Debug, Clone, Copy)]
pub struct AngularVelocity(pub Vec3);

#[derive(Component, Debug, Clone, Copy)]
pub struct OrbitalVelocity {
        pub origin: Vec3,
        pub velocity: Vec3,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Force(pub Vec3);

#[derive(Component, Debug, Clone, Copy)]
pub struct Mass(pub f32);

#[derive(Component, Debug, Clone, Copy)]
pub struct Parent(pub Entity);

#[derive(Component, Debug, Clone, Copy)]
pub struct RelativeTransform(pub Transform);

#[derive(Component, Debug, Clone, Copy)]
pub struct OldTransform(pub Transform);

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct ProjectionCamera {
        fovy: f32,
        zoom: f32,
        near: f32,
        far: f32,
}

impl ProjectionCamera {
        pub fn new(fovy: f32, zoom: f32, near: f32, far: f32) -> Self {
                Self { fovy, zoom, near, far }
        }

        pub fn calc_proj_matrix(&self, aspect_ratio: f32) -> Mat4 {
                glm::perspective_rh_zo(aspect_ratio, self.fovy / self.zoom, self.near, self.far)
        }
}

#[derive(Component, Debug)]
pub struct LightEmitter {
        pub color: Vec3,
}
