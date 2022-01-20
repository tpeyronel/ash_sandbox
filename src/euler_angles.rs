use crate::my_glm::{UnitQuat, Vec3};

const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.0001;
const PITCH_MIN: f32 = -PITCH_MAX;

#[derive(Debug, Clone)]
pub struct EulerAngles {
        pitch: f32,
        yaw: f32,
        roll: f32,
}

impl EulerAngles {
        pub fn new(pitch: f32, yaw: f32, roll: f32) -> Self {
                Self { pitch, yaw, roll }
        }

        pub fn to_quat(&self) -> UnitQuat {
                // UnitQuat::from_euler_angles();

                // let cy = f32::cos(self.yaw * 0.5);
                // let sy = f32::sin(self.yaw * 0.5);
                // let cp = f32::cos(self.pitch * 0.5);
                // let sp = f32::sin(self.pitch * 0.5);
                // let cr = f32::cos(self.roll * 0.5);
                // let sr = f32::sin(self.roll * 0.5);

                // UnitQuat::new_unchecked(Quat::new(
                //         cr * cp * cy + sr * sp * sy,
                //         cr * sp * cy + sr * cp * sy,
                //         cr * cp * sy - sr * sp * cy,
                //         sr * cp * cy - cr * sp * sy,
                // ))

                UnitQuat::from_axis_angle(&Vec3::y_axis(), self.yaw)
                        * UnitQuat::from_axis_angle(&Vec3::x_axis(), self.pitch)
                        * UnitQuat::from_axis_angle(&Vec3::z_axis(), self.roll)
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

        pub fn set_roll(&mut self, angle: f32) {
                self.roll = angle;
        }

        pub fn set_pitch(&mut self, angle: f32) {
                self.pitch = angle;
        }

        pub fn set_yaw(&mut self, angle: f32) {
                self.yaw = angle;
        }

        pub fn roll_by(&mut self, angle: f32) {
                self.roll += angle;

                if self.roll > 180.0f32.to_radians() {
                        self.roll -= 360.0f32.to_radians();
                } else if self.roll < -180.0f32.to_radians() {
                        self.roll += 360.0f32.to_radians();
                }
        }

        pub fn pitch_by(&mut self, angle: f32) {
                self.pitch += angle;

                self.pitch = f32::clamp(self.pitch, PITCH_MIN, PITCH_MAX);
        }

        pub fn yaw_by(&mut self, angle: f32) {
                self.yaw += angle;

                if self.yaw > 180.0f32.to_radians() {
                        self.yaw -= 360.0f32.to_radians();
                } else if self.yaw < -180.0f32.to_radians() {
                        self.yaw += 360.0f32.to_radians();
                }
        }
}
