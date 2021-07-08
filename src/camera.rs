use crate::my_glm::*;
#[allow(unused_imports)]
use log::info;

const CLAMP_PITCH: bool = true;
const PITCH_CLAMP_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.0001;
const PITCH_CLAMP_MIN: f32 = -PITCH_CLAMP_MAX;
pub struct Camera {
        pos: Vec3,

        pitch: f32,
        yaw: f32,
        roll: f32,
        fovy: f32,
        zoom: f32,
        aspect_ratio: f32,
        near: f32,
        far: f32,

        orien: UnitQuat,
        hor_orien: UnitQuat,
        view: Mat4,
        proj: Mat4,
        orien_outdated: bool,
        hor_orien_outdated: bool,
        view_outdated: bool,
        proj_outdated: bool,
}

impl Camera {
        pub fn new(
                pos: &Vec3,
                pitch: f32,
                yaw: f32,
                roll: f32,
                fovy: f32,
                zoom: f32,
                width: u32,
                height: u32,
                near: f32,
                far: f32,
        ) -> Self {
                assert!(zoom > f32::EPSILON, "zoom can't be zero!");

                let aspect_ratio = width as f32 / height as f32;
                let orien = Self::calc_orientation(pitch, yaw, roll);
                let hor_orien = Self::calc_hor_orien(&orien);
                let view = Self::calc_view_matrix(pos, &orien);
                let proj = Self::calc_proj_matrix(fovy, zoom, aspect_ratio, near, far);

                Self {
                        pos: *pos,
                        pitch,
                        yaw,
                        roll,
                        fovy,
                        zoom,
                        aspect_ratio,
                        near,
                        far,
                        orien,
                        hor_orien,
                        view,
                        proj,
                        orien_outdated: false,
                        hor_orien_outdated: false,
                        view_outdated: false,
                        proj_outdated: false,
                }
        }

        pub fn pitch(&self) -> f32 {
                self.pitch
        }

        pub fn yaw(&self) -> f32 {
                self.yaw
        }

        pub fn roll(&self) -> f32 {
                self.roll
        }

        pub fn zoom(&self) -> f32 {
                self.zoom
        }

        pub fn orien(&mut self) -> &UnitQuat {
                if self.orien_outdated {
                        self.orien = Self::calc_orientation(self.pitch, self.yaw, self.roll);
                        self.orien_outdated = false;
                }

                &self.orien
        }

        pub fn hor_orien(&mut self) -> &UnitQuat {
                if self.hor_orien_outdated {
                        self.hor_orien = Self::calc_hor_orien(self.orien());
                        self.hor_orien_outdated = false;
                }

                &self.hor_orien
        }

        pub fn view(&mut self) -> &Mat4 {
                if self.view_outdated {
                        self.orien();
                        self.view = Self::calc_view_matrix(&self.pos, &self.orien);
                        self.view_outdated = false;
                }

                &self.view
        }

        pub fn proj(&mut self) -> &Mat4 {
                if self.proj_outdated {
                        self.proj =
                                Self::calc_proj_matrix(self.fovy, self.zoom, self.aspect_ratio, self.near, self.far);
                        self.proj_outdated = false;
                }

                &self.proj
        }

        pub fn set_roll(&mut self, angle: f32) {
                self.roll = angle;

                self.hor_orien_outdated = true;
                self.orien_outdated = true;
                self.view_outdated = true;
        }

        pub fn set_pitch(&mut self, angle: f32) {
                self.pitch = angle;

                self.hor_orien_outdated = true;
                self.orien_outdated = true;
                self.view_outdated = true;
        }

        pub fn set_yaw(&mut self, angle: f32) {
                self.yaw = angle;

                self.hor_orien_outdated = true;
                self.orien_outdated = true;
                self.view_outdated = true;
        }

        pub fn roll_by(&mut self, angle: f32) {
                self.roll += angle;

                if self.roll > 180.0f32.to_radians() {
                        self.roll -= 360.0f32.to_radians();
                } else if self.roll < -180.0f32.to_radians() {
                        self.roll += 360.0f32.to_radians();
                }

                self.hor_orien_outdated = true;
                self.orien_outdated = true;
                self.view_outdated = true;
        }

        pub fn pitch_by(&mut self, angle: f32) {
                self.pitch += angle / self.zoom;

                if CLAMP_PITCH {
                        self.pitch = f32::clamp(self.pitch, PITCH_CLAMP_MIN, PITCH_CLAMP_MAX);
                } else {
                        if self.pitch > 180.0f32.to_radians() {
                                self.pitch -= 360.0f32.to_radians();
                        } else if self.pitch < -180.0f32.to_radians() {
                                self.pitch += 360.0f32.to_radians();
                        }
                }

                self.hor_orien_outdated = true;
                self.orien_outdated = true;
                self.view_outdated = true;
        }

        pub fn yaw_by(&mut self, angle: f32) {
                self.yaw += angle / self.zoom;

                if self.yaw > 180.0f32.to_radians() {
                        self.yaw -= 360.0f32.to_radians();
                } else if self.yaw < -180.0f32.to_radians() {
                        self.yaw += 360.0f32.to_radians();
                }

                self.hor_orien_outdated = true;
                self.orien_outdated = true;
                self.view_outdated = true;
        }

        pub fn set_pos(&mut self, pos: &Vec3) {
                self.pos = *pos;
                self.view_outdated = true;
        }

        pub fn translate(&mut self, t: &Vec3) {
                self.pos += *t;
                self.view_outdated = true;
        }

        pub fn set_zoom(&mut self, zoom: f32) {
                self.zoom = zoom;
                self.proj_outdated = true;
        }

        pub fn zoom_by(&mut self, zoom: f32) {
                self.zoom = f32::max(self.zoom + zoom, 1.0);
                self.proj_outdated = true;
        }

        pub fn on_window_resize(&mut self, width: u32, height: u32) {
                self.aspect_ratio = width as f32 / height as f32;
                self.proj_outdated = true;
        }

        fn calc_orientation(pitch: f32, yaw: f32, roll: f32) -> UnitQuat {
                let fpitch2 = pitch / 2.0;
                let fyaw2 = yaw / 2.0;
                let froll2 = roll / 2.0;

                let qyaw = UnitQuat::new_unchecked(Quat::new(fyaw2.cos(), 0.0, fyaw2.sin(), 0.0));
                let qpitch = UnitQuat::new_unchecked(Quat::new(fpitch2.cos(), -fpitch2.sin(), 0.0, 0.0));
                let qroll = UnitQuat::new_unchecked(Quat::new(froll2.cos(), 0.0, 0.0, -froll2.sin()));

                qyaw * qpitch * qroll
        }

        fn calc_hor_orien(orien: &UnitQuat) -> UnitQuat {
                UnitQuat::new_normalize(Quat::new(orien.as_vector().w, 0.0, orien.as_vector().y, 0.0))
        }

        fn calc_view_matrix(pos: &Vec3, orien: &UnitQuat) -> Mat4 {
                (Mat4::new_translation(pos) * orien.to_homogeneous())
                        .try_inverse()
                        .expect("Couldn't invert camera ViewMatrix!")
        }

        fn calc_proj_matrix(fovy: f32, zoom: f32, aspect_ratio: f32, near: f32, far: f32) -> Mat4 {
                glm::perspective_lh_zo(aspect_ratio, fovy / zoom, near, far)
        }
}
