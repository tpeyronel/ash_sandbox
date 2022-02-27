use std::ops::Mul;

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
        pub translation: Vec3,
        pub rotation: UnitQuat,
        pub scale: Vec3,
}

impl Default for Transform {
        fn default() -> Self {
                Self::identity()
        }
}

impl Transform {
        pub fn interp(&self, other: &Self, t: f32) -> Self {
                Self {
                        translation: Vec3::lerp(&self.translation, &other.translation, t),
                        rotation: UnitQuat::try_slerp(&self.rotation, &other.rotation, t, 0.0)
                                .unwrap_or_else(|| UnitQuat::nlerp(&self.rotation, &other.rotation, t)),
                        scale: Vec3::lerp(&self.scale, &other.scale, t),
                }
        }

        pub fn to_matrix(&self) -> Mat4 {
                Mat4::new_translation(&self.translation)
                        * UnitQuat::to_homogeneous(&self.rotation)
                        * Mat4::new_nonuniform_scaling(&self.scale)
        }

        pub fn from_translation(translation: Vec3) -> Self {
                Self {
                        translation,
                        ..Self::identity()
                }
        }

        pub fn from_rotation(rotation: UnitQuat) -> Self {
                Self {
                        rotation,
                        ..Self::identity()
                }
        }

        pub fn from_scale(scale: Vec3) -> Self {
                Self {
                        scale,
                        ..Self::identity()
                }
        }

        pub fn identity() -> Self {
                Self {
                        translation: Vec3::from_element(0.0),
                        rotation: UnitQuat::identity(),
                        scale: Vec3::from_element(1.0),
                }
        }
}

impl Mul<Transform> for Transform {
        type Output = Transform;

        fn mul(self, rhs: Transform) -> Self::Output {
                Self {
                        translation: self.translation + self.scale.component_mul(&(self.rotation * rhs.translation)),
                        rotation: self.rotation * rhs.rotation,
                        scale: self.scale.component_mul(&rhs.scale),
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

#[derive(Component, Debug, Clone)]
pub struct Children(pub Vec<Entity>);

#[derive(Component, Debug, Clone, Copy)]
pub struct GlobalTransform(pub Transform);

#[derive(Component, Debug, Clone, Copy)]
pub struct PreviousGlobalTransform(pub Transform);

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
