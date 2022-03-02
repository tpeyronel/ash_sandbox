use std::convert::TryInto;

use bevy_ecs::change_detection::Mut;

use crate::{
        asset_manager::Material,
        components::{DirectionalLight, PointLight, Spotlight, Transform},
        euler_angles::EulerAngles,
        my_glm::*,
};

pub fn imgui_vec3<T: AsRef<str>>(ui: &imgui::Ui<'_>, label: T, min: f32, max: f32, mut vec: Vec3) -> Option<Vec3> {
        if imgui::Slider::new(label, min, max).build_array(&ui, (&mut vec).into()) {
                Some(vec)
        } else {
                None
        }
}

pub trait ImguiObject {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>);
}

impl ImguiObject for Mut<'_, Transform> {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>) {
                if let Some(translation) = imgui_vec3(&ui, "translation", -2.5, 2.5, self.translation) {
                        self.translation = translation;
                }
        }
}

impl ImguiObject for Mut<'_, EulerAngles> {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>) {
                let mut pitch = self.pitch();
                if imgui::AngleSlider::new("pitch")
                        .range_degrees(-90.0, 90.0)
                        .build(&ui, &mut pitch)
                {
                        self.set_pitch(pitch);
                }

                let mut yaw = self.yaw();
                if imgui::AngleSlider::new("yaw")
                        .range_degrees(-180.0, 180.0)
                        .build(&ui, &mut yaw)
                {
                        self.set_yaw(yaw);
                }

                let mut roll = self.roll();
                if imgui::AngleSlider::new("roll")
                        .range_degrees(-180.0, 180.0)
                        .build(&ui, &mut roll)
                {
                        self.set_roll(roll);
                }
        }
}

impl ImguiObject for Mut<'_, DirectionalLight> {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>) {
                if ui.button("disable") {
                        self.color = Vec3::from_element(0.0);
                }

                let mut direction = self.direction;
                if imgui::Slider::new("direction", -1.0, 1.0).build_array(&ui, (&mut direction).into()) {
                        self.direction = direction;
                }

                let mut color: [f32; 3] = self.color.try_into().unwrap();
                if imgui::ColorEdit::new("color", &mut color).build(&ui) {
                        self.color = Vec3::from_column_slice(&color);
                }
        }
}

impl ImguiObject for Mut<'_, PointLight> {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>) {
                if ui.button("disable") {
                        self.color = Vec3::from_element(0.0);
                }

                let mut color: [f32; 3] = self.color.try_into().unwrap();
                if imgui::ColorEdit::new("color", &mut color).build(&ui) {
                        self.color = Vec3::from_column_slice(&color);
                }

                let mut kc = self.kc;
                if imgui::Slider::new("constant", 0.0, 1.0).build(&ui, &mut kc) {
                        self.kc = kc;
                }

                let mut kl = self.kl;
                if imgui::Slider::new("linear", 0.0, 1.0).build(&ui, &mut kl) {
                        self.kl = kl;
                }

                let mut kq = self.kq;
                if imgui::Slider::new("quadratic", 0.0, 1.0)
                        .flags(imgui::SliderFlags::LOGARITHMIC)
                        .build(&ui, &mut kq)
                {
                        self.kq = kq;
                }
        }
}

impl ImguiObject for Mut<'_, Spotlight> {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>) {
                if ui.button("disable") {
                        self.color = Vec3::from_element(0.0);
                }

                let mut color: [f32; 3] = self.color.try_into().unwrap();
                if imgui::ColorEdit::new("color", &mut color).build(&ui) {
                        self.color = Vec3::from_column_slice(&color);
                }

                let mut angle = self.radius_angle;
                if imgui::AngleSlider::new("cutoff angle")
                        .range_degrees(0.0, 90.0)
                        .build(&ui, &mut angle)
                {
                        self.radius_angle = angle;
                }

                let mut inner_circle = self.inner_radius_percentage;
                if imgui::Slider::new("inner circle", 0.0, 1.0).build(&ui, &mut inner_circle) {
                        self.inner_radius_percentage = inner_circle;
                }

                let mut kc = self.kc;
                if imgui::Slider::new("constant", 0.0, 1.0).build(&ui, &mut kc) {
                        self.kc = kc;
                }

                let mut kl = self.kl;
                if imgui::Slider::new("linear", 0.0, 1.0).build(&ui, &mut kl) {
                        self.kl = kl;
                }

                let mut kq = self.kq;
                if imgui::Slider::new("quadratic", 0.0, 1.0)
                        .flags(imgui::SliderFlags::LOGARITHMIC)
                        .build(&ui, &mut kq)
                {
                        self.kq = kq;
                }
        }
}

impl ImguiObject for Material {
        fn build_imgui_ui(&mut self, ui: &imgui::Ui<'_>) {
                imgui::Slider::new("shininess", 0.0f32, 256.0).build(&ui, &mut self.shininess);
                imgui::Slider::new("ambient strength", 0.0f32, 1.0).build(&ui, &mut self.ambient_strength);
                imgui::Slider::new("specular strength", 0.0f32, 1.0).build(&ui, &mut self.specular_strength);
                imgui::Slider::new("diffuse strength", 0.0f32, 1.0).build(&ui, &mut self.diffuse_strength);
        }
}
