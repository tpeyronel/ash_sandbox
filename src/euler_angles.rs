use crate::my_glm::{UnitQuat, Vec3};

const PITCH_MAX: f32 = std::f32::consts::FRAC_PI_2 - 0.0001;
const PITCH_MIN: f32 = -PITCH_MAX;

#[derive(Debug, Clone)]
pub struct EulerAngles {
        angles: [f32; 3],
}

impl EulerAngles {
        pub fn new(pitch: f32, yaw: f32, roll: f32) -> Self {
                Self { angles: [pitch, yaw, roll] }
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

                UnitQuat::from_axis_angle(&Vec3::y_axis(), self.yaw())
                        * UnitQuat::from_axis_angle(&Vec3::x_axis(), self.pitch())
                        * UnitQuat::from_axis_angle(&Vec3::z_axis(), self.roll())
        }

        pub fn pitch(&self) -> f32 {
                self.angles[0]
        }

        pub fn yaw(&self) -> f32 {
                self.angles[1]
        }

        pub fn roll(&self) -> f32 {
                self.angles[2]
        }

        pub fn as_slice(&self) -> &[f32] {
                &self.angles
        }

        pub fn as_slice_mut(&mut self) -> &mut [f32] {
                &mut self.angles
        }

        pub fn as_array(&self) -> &[f32; 3] {
                &self.angles
        }

        pub fn as_array_mut(&mut self) -> &mut [f32; 3] {
                &mut self.angles
        }

        pub fn set_pitch(&mut self, pitch: f32) {
                self.angles[0] = pitch;
        }

        pub fn set_yaw(&mut self, yaw: f32) {
                self.angles[1] = yaw;
        }

        pub fn set_roll(&mut self, roll: f32) {
                self.angles[2] = roll;
        }

        pub fn pitch_by(&mut self, pitch: f32) {
                self.angles[0] += pitch;

                self.angles[0] = f32::clamp(self.angles[0], PITCH_MIN, PITCH_MAX);
        }

        pub fn yaw_by(&mut self, yaw: f32) {
                self.angles[1] += yaw;

                if self.angles[1] > 180.0f32.to_radians() {
                        self.angles[1] -= 360.0f32.to_radians();
                } else if self.angles[1] < -180.0f32.to_radians() {
                        self.angles[1] += 360.0f32.to_radians();
                }
        }

        pub fn roll_by(&mut self, roll: f32) {
                self.angles[2] += roll;

                if self.angles[2] > 180.0f32.to_radians() {
                        self.angles[2] -= 360.0f32.to_radians();
                } else if self.angles[2] < -180.0f32.to_radians() {
                        self.angles[2] += 360.0f32.to_radians();
                }
        }
}
