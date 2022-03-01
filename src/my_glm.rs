use glm::{TVec3, TVec4};
use nalgebra::{Scalar, Unit};

extern crate nalgebra_glm as glm;

#[allow(dead_code)]
pub type Quat = glm::Quat;
#[allow(dead_code)]
pub type UnitQuat = nalgebra::UnitQuaternion<f32>;

#[allow(dead_code)]
pub type Vec1 = glm::Vec1;
#[allow(dead_code)]
pub type Vec2 = glm::Vec2;
#[allow(dead_code)]
pub type Vec3 = glm::Vec3;
#[allow(dead_code)]
pub type Vec4 = glm::Vec4;

#[allow(dead_code)]
pub type UnitVec1 = Unit<glm::Vec1>;
#[allow(dead_code)]
pub type UnitVec2 = Unit<glm::Vec2>;
#[allow(dead_code)]
pub type UnitVec3 = Unit<glm::Vec3>;
#[allow(dead_code)]
pub type UnitVec4 = Unit<glm::Vec4>;

#[allow(dead_code)]
pub type Vec1u = glm::UVec1;
#[allow(dead_code)]
pub type Vec2u = glm::UVec2;
#[allow(dead_code)]
pub type Vec3u = glm::UVec3;
#[allow(dead_code)]
pub type Vec4u = glm::UVec4;

#[allow(dead_code)]
pub type Vec1i = glm::IVec1;
#[allow(dead_code)]
pub type Vec2i = glm::IVec2;
#[allow(dead_code)]
pub type Vec3i = glm::IVec3;
#[allow(dead_code)]
pub type Vec4i = glm::IVec4;

#[allow(dead_code)]
pub type Mat2 = glm::Mat2;
#[allow(dead_code)]
pub type Mat3 = glm::Mat3;
#[allow(dead_code)]
pub type Mat4 = glm::Mat4;

pub trait VectorUtil<T: Scalar> {
        fn new_position(pos: &TVec3<T>) -> Self;
        fn new_direction(dir: &TVec3<T>) -> Self;
        fn new_vec3_and_w(vec: &TVec3<T>, w: T) -> Self;
}

// impl <T: Scalar> VectorUtil<T> for TVec4<T> {
//     fn new_position(pos: TVec3<T>) -> Self {
//             Self::new(pos.x, pos.y, pos.z, nalgebra::convert(1.0f64))
//     }
// }

impl VectorUtil<f32> for TVec4<f32> {
        fn new_position(pos: &TVec3<f32>) -> Self {
                Self::new(pos.x, pos.y, pos.z, 1.0)
        }

        fn new_direction(dir: &TVec3<f32>) -> Self {
                Self::new(dir.x, dir.y, dir.z, 0.0)
        }

        fn new_vec3_and_w(vec: &TVec3<f32>, w: f32) -> Self {
                Self::new(vec.x, vec.y, vec.z, w)
        }
}

impl VectorUtil<f64> for TVec4<f64> {
        fn new_position(pos: &TVec3<f64>) -> Self {
                Self::new(pos.x, pos.y, pos.z, 1.0)
        }

        fn new_direction(dir: &TVec3<f64>) -> Self {
                Self::new(dir.x, dir.y, dir.z, 0.0)
        }

        fn new_vec3_and_w(vec: &TVec3<f64>, w: f64) -> Self {
                Self::new(vec.x, vec.y, vec.z, w)
        }
}
