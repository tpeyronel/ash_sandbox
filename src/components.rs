use std::{ops::Mul, time::Instant};

use bevy_ecs::prelude::*;

use crate::my_glm::*;

pub struct Player(pub Entity);

pub struct ActiveCamera(pub Entity);

pub struct TickTime(pub f32);

pub struct DeltaTimeAccumulator(pub f32);

pub struct UpdateBegin(pub Instant);

pub struct InterpScalar(pub f32);

pub struct ImguiWantCaptureMouse(pub bool);
pub struct ImguiWantCaptureKeyboard(pub bool);

#[derive(Component, Debug, Clone, Copy)]
pub struct Transform {
        pub translation: Vec3,
        pub rotation: Quat,
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
                        translation: Vec3::lerp(self.translation, other.translation, t),
                        rotation: Quat::slerp(
                                if Quat::dot(self.rotation, other.rotation) >= 0.0 {
                                        self.rotation
                                } else {
                                        -self.rotation
                                },
                                other.rotation,
                                t,
                        ),
                        scale: Vec3::lerp(self.scale, other.scale, t),
                }
        }

        pub fn as_matrix(&self) -> Mat4 {
                // Mat4::from_translation(self.translation) * Mat4::from_quat(self.rotation) * Mat4::from_scale(self.scale)
                Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
        }

        pub fn from_translation(translation: Vec3) -> Self {
                Self {
                        translation,
                        ..Self::identity()
                }
        }

        pub fn from_rotation(rotation: Quat) -> Self {
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

        pub fn from_mat4(mat: &Mat4) -> Self {
                let (scale, rotation, translation) = mat.to_scale_rotation_translation();

                Self {
                        translation,
                        rotation,
                        scale,
                }
        }

        pub fn identity() -> Self {
                Self {
                        translation: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                        scale: Vec3::ONE,
                }
        }
}

impl Mul<Transform> for Transform {
        type Output = Transform;

        fn mul(self, rhs: Transform) -> Self::Output {
                Self {
                        translation: self.translation + (self.rotation * (self.scale * rhs.translation)),
                        rotation: self.rotation * rhs.rotation,
                        scale: self.scale * rhs.scale,
                        // scale:  (rhs.rotation.inverse() * self.scale).abs() * rhs.scale,
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
                Mat4::perspective_rh(self.fovy / self.zoom, aspect_ratio, self.near, self.far)
        }
}

#[derive(Component, Debug)]
pub struct PointLight {
        pub color: Vec3,
        pub kc: f32,
        pub kl: f32,
        pub kq: f32,
}

#[derive(Component, Debug)]
pub struct DirectionalLight {
        pub direction: Vec3,
        pub color: Vec3,
}

#[derive(Component, Debug)]
pub struct Spotlight {
        pub radius_angle: f32,
        pub inner_radius_percentage: f32,
        pub color: Vec3,
        pub kc: f32,
        pub kl: f32,
        pub kq: f32,
}

#[derive(Component, Debug)]
pub struct Billboard;
