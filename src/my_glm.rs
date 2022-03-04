#[allow(dead_code)]
pub type Quat = glam::Quat;

#[allow(dead_code)]
pub type Vec2 = glam::Vec2;
#[allow(dead_code)]
pub type Vec3 = glam::Vec3;
#[allow(dead_code)]
pub type Vec4 = glam::Vec4;

#[allow(dead_code)]
pub type Vec2u = glam::UVec2;
#[allow(dead_code)]
pub type Vec3u = glam::UVec3;
#[allow(dead_code)]
pub type Vec4u = glam::UVec4;

#[allow(dead_code)]
pub type Vec2i = glam::IVec2;
#[allow(dead_code)]
pub type Vec3i = glam::IVec3;
#[allow(dead_code)]
pub type Vec4i = glam::IVec4;

#[allow(dead_code)]
pub type Mat2 = glam::Mat2;
#[allow(dead_code)]
pub type Mat3 = glam::Mat3;
#[allow(dead_code)]
pub type Mat4 = glam::Mat4;

// pub trait VectorUtil<T: Scalar> {
//         fn new_position(pos: &TVec3<T>) -> Self;
//         fn new_direction(dir: &TVec3<T>) -> Self;
//         fn new_vec3_and_w(vec: &TVec3<T>, w: T) -> Self;
// }

// impl VectorUtil<f32> for TVec4<f32> {
//         fn new_position(pos: &TVec3<f32>) -> Self {
//                 Self::new(pos.x, pos.y, pos.z, 1.0)
//         }

//         fn new_direction(dir: &TVec3<f32>) -> Self {
//                 Self::new(dir.x, dir.y, dir.z, 0.0)
//         }

//         fn new_vec3_and_w(vec: &TVec3<f32>, w: f32) -> Self {
//                 Self::new(vec.x, vec.y, vec.z, w)
//         }
// }

// impl VectorUtil<f64> for TVec4<f64> {
//         fn new_position(pos: &TVec3<f64>) -> Self {
//                 Self::new(pos.x, pos.y, pos.z, 1.0)
//         }

//         fn new_direction(dir: &TVec3<f64>) -> Self {
//                 Self::new(dir.x, dir.y, dir.z, 0.0)
//         }

//         fn new_vec3_and_w(vec: &TVec3<f64>, w: f64) -> Self {
//                 Self::new(vec.x, vec.y, vec.z, w)
//         }
// }

pub trait WorldDirections {
        const FORWARD: Vec3;
        const BACKWARD: Vec3;
        const RIGHT: Vec3;
        const LEFT: Vec3;
        const UP: Vec3;
        const DOWN: Vec3;
}

impl WorldDirections for Vec3 {
        const FORWARD: Vec3 = glam::const_vec3!([0.0, 0.0, -1.0]);

        const BACKWARD: Vec3 = glam::const_vec3!([0.0, 0.0, 1.0]);

        const RIGHT: Vec3 = glam::const_vec3!([1.0, 0.0, 0.0]);

        const LEFT: Vec3 = glam::const_vec3!([-1.0, 0.0, 0.0]);

        const UP: Vec3 = glam::const_vec3!([0.0, 1.0, 0.0]);

        const DOWN: Vec3 = glam::const_vec3!([0.0, -1.0, 0.0]);
}

pub trait FaceTowards {
        fn face_towards(eye: Vec3, target: Vec3, up: Vec3) -> Self;
}

impl FaceTowards for Mat3 {
        fn face_towards(eye: Vec3, target: Vec3, up: Vec3) -> Self {
                let backward = Vec3::normalize(eye - target);
                let right = up.cross(backward).normalize();
                let up = backward.cross(right);

                Mat3::from_cols(right, up, backward)
        }
}

impl FaceTowards for Quat {
        fn face_towards(eye: Vec3, target: Vec3, up: Vec3) -> Self {
                Quat::from_mat3(&Mat3::face_towards(eye, target, up))
        }
}
