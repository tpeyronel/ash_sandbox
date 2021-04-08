use crate::my_glm::*;
#[allow(unused_imports)]
use log::info;

pub struct Camera {
        pos: Vec3,
        pitch: f32,
        yaw: f32,
        roll: f32,
        orien: UnitQuat,
        hor_orien: UnitQuat,
        fov: f32,
        zoom: f32,
        aspect_ratio: f32,
        near: f32,
        far: f32,

        view: Mat4,
        proj: Mat4,
}

impl Camera {
        pub fn new(
                pos: &Vec3,
                pitch: f32,
                yaw: f32,
                roll: f32,
                fov: f32,
                zoom: f32,
                width: u32,
                height: u32,
                near: f32,
                far: f32,
        ) -> Self {
                assert!(zoom > f32::EPSILON, "zoom can't be zero!");

                let aspect_ratio = width as f32 / height as f32;
                let hor_orien = Self::calc_hor_orientation(yaw);
                let orien = Self::calc_orientation(&hor_orien, yaw, roll);
                let view = Self::calc_view_matrix(pos, &orien);

                Self {
                        pos: *pos,
                        pitch,
                        yaw,
                        roll,
                        orien,
                        hor_orien,
                        fov,
                        zoom,
                        view,
                        aspect_ratio,
                        near,
                        far,
                        proj: Self::calc_proj_matrix(fov, zoom, aspect_ratio, near, far),
                }
        }

        pub fn get_pitch(&self) -> f32 {
                self.pitch
        }

        pub fn get_yaw(&self) -> f32 {
                self.yaw
        }

        pub fn get_roll(&self) -> f32 {
                self.roll
        }

        pub fn get_orientation(&self) -> &UnitQuat {
                &self.orien
        }

        pub fn get_hor_orientation(&self) -> &UnitQuat {
                &self.hor_orien
        }

        pub fn get_view(&self) -> &Mat4 {
                &self.view
        }

        pub fn get_proj(&self) -> &Mat4 {
                &self.proj
        }

        pub fn set_euler_angles(&mut self, pitch: f32, yaw: f32, roll: f32) {
                self.pitch = pitch;
                self.yaw = yaw;
                self.roll = roll;
                self.hor_orien = Self::calc_hor_orientation(self.yaw);
                self.orien = Self::calc_orientation(&self.hor_orien, self.pitch, self.roll);
                self.view = Self::calc_view_matrix(&self.pos, &self.orien);
        }

        pub fn rotate_euler_angles(&mut self, pitch: f32, yaw: f32, roll: f32) {
                self.pitch += pitch * 0.001;
                self.yaw += yaw * 0.001;
                self.roll += roll * 0.001;
                self.hor_orien = Self::calc_hor_orientation(self.yaw);
                self.orien = Self::calc_orientation(&self.hor_orien, self.pitch, self.roll);
                self.view = Self::calc_view_matrix(&self.pos, &self.orien);
        }

        pub fn set_pos(&mut self, pos: &Vec3) {
                self.pos = *pos;
                self.hor_orien = Self::calc_hor_orientation(self.yaw);
                self.orien = Self::calc_orientation(&self.hor_orien, self.pitch, self.roll);
                self.view = Self::calc_view_matrix(&self.pos, &self.orien);
        }

        pub fn translate(&mut self, t: &Vec3) {
                self.pos += *t;
                self.hor_orien = Self::calc_hor_orientation(self.yaw);
                self.orien = Self::calc_orientation(&self.hor_orien, self.pitch, self.roll);
                self.view = Self::calc_view_matrix(&self.pos, &self.orien);
        }

        pub fn set_zoom(&mut self, zoom: f32) {
                self.zoom = zoom;
                self.proj = Self::calc_proj_matrix(self.fov, self.zoom, self.aspect_ratio, self.near, self.far);
        }

        pub fn on_window_resize(&mut self, width: u32, height: u32) {
                self.aspect_ratio = width as f32 / height as f32;
                self.proj = Self::calc_proj_matrix(self.fov, self.zoom, self.aspect_ratio, self.near, self.far);
        }

        fn calc_hor_orientation(yaw: f32) -> UnitQuat {
                UnitQuat::new_unchecked(Quat::new(yaw.cos(), 0.0, yaw.sin(), 0.0))
        }

        fn calc_orientation(hor_orien: &UnitQuat, pitch: f32, roll: f32) -> UnitQuat {
                let qpitch = UnitQuat::new_unchecked(Quat::new(pitch.cos(), -pitch.sin(), 0.0, 0.0));
                let qroll = UnitQuat::new_unchecked(Quat::new(roll.cos(), 0.0, 0.0, -roll.sin()));

                (*hor_orien) * qpitch * qroll
        }

        fn calc_view_matrix(pos: &Vec3, orien: &UnitQuat) -> Mat4 {
                (Mat4::new_translation(pos) * orien.to_homogeneous())
                        .try_inverse()
                        .expect("Couldn't invert camera ViewMatrix!")
        }

        fn calc_proj_matrix(_fov: f32, _zoom: f32, aspect_ratio: f32, near: f32, far: f32) -> Mat4 {
                glm::perspective_lh_zo(aspect_ratio, 90.0f32.to_radians()/* (fov.tan() / zoom).atan() */, near, far)
        }
}
