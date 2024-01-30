use std::ops::{Deref, DerefMut};

use crate::{
        asset_manager::Material,
        components::{DirectionalLight, PointLight, Spotlight, Transform},
        euler_angles::EulerAngles,
        my_glm::*,
};

pub fn imgui_vec3<T: AsRef<str>>(ui: &imgui::Ui, label: T, min: f32, max: f32, vec: Vec3) -> Option<Vec3> {
        let mut v: [f32; 3] = vec.into();
        if ui.slider_config(label, min, max).build_array(&mut v) {
                Some(Vec3::from_slice(&v))
        } else {
                None
        }
}

pub fn transform(ui: &imgui::Ui, mut transform: Transform) -> Option<Transform> {
        let mut changed = false;

        let mut translation: [f32; 3] = transform.translation.into();
        if imgui::Drag::new("translation")
                .speed(0.1)
                .build_array(ui, &mut translation)
        {
                transform.translation = Vec3::from_slice(&translation);
                changed = true;
        }

        let euler_angles = EulerAngles::from_quat(transform.rotation);
        if let Some(xyz) = imgui_vec3(
                ui,
                "rotation",
                -180.0,
                180.0,
                Vec3::new(
                        euler_angles.pitch().to_degrees(),
                        euler_angles.yaw().to_degrees(),
                        euler_angles.roll().to_degrees(),
                ),
        ) {
                transform.rotation = EulerAngles::new(
                        xyz.x.clamp(-90.0, 90.0).to_radians(),
                        xyz.y.to_radians(),
                        xyz.z.to_radians(),
                )
                .to_quat();
                changed = true;
        }

        if let Some(scale) = imgui_vec3(ui, "scale", 0.0, 5.0, transform.scale) {
                transform.scale = scale;
                changed = true;
        }

        if changed {
                Some(transform)
        } else {
                None
        }
}

pub fn transform_mut<T>(ui: &imgui::Ui, transform: &mut T)
where
        T: Deref<Target = Transform> + DerefMut,
{
        if let Some(t) = self::transform(ui, **transform) {
                **transform = t;
        }
}

pub fn euler_angles_mut<T>(ui: &imgui::Ui, euler_angles: &mut T)
where
        T: Deref<Target = EulerAngles> + DerefMut,
{
        let mut pitch = euler_angles.pitch();
        if imgui::AngleSlider::new("pitch")
                .range_degrees(-90.0, 90.0)
                .build(ui, &mut pitch)
        {
                euler_angles.set_pitch(pitch);
        }

        let mut yaw = euler_angles.yaw();
        if imgui::AngleSlider::new("yaw")
                .range_degrees(-180.0, 180.0)
                .build(ui, &mut yaw)
        {
                euler_angles.set_yaw(yaw);
        }

        let mut roll = euler_angles.roll();
        if imgui::AngleSlider::new("roll")
                .range_degrees(-180.0, 180.0)
                .build(ui, &mut roll)
        {
                euler_angles.set_roll(roll);
        }
}

pub fn dir_light_mut<T>(ui: &imgui::Ui, dir_light: &mut T)
where
        T: Deref<Target = DirectionalLight> + DerefMut,
{
        if ui.button("disable") {
                dir_light.color = Vec3::splat(0.0);
        }

        let mut direction: [f32; 3] = dir_light.direction.into();
        if ui.slider_config("direction", -1.0, 1.0f32).build_array(&mut direction) {
                dir_light.direction = Vec3::from_slice(&direction);
        }

        let mut color: [f32; 3] = dir_light.color.into();
        if ui.color_edit3("color", &mut color) {
                dir_light.color = Vec3::from_slice(&color);
        }
}

pub fn point_light_mut<T>(ui: &imgui::Ui, point_light: &mut T)
where
        T: Deref<Target = PointLight> + DerefMut,
{
        if ui.button("disable") {
                point_light.color = Vec3::splat(0.0);
        }

        let mut color: [f32; 3] = point_light.color.into();
        if ui.color_edit3("color", &mut color) {
                point_light.color = Vec3::from_slice(&color);
        }

        let mut kc = point_light.kc;
        if ui.slider("constant", 0.0, 1.0, &mut kc) {
                point_light.kc = kc;
        }

        let mut kl = point_light.kl;
        if ui.slider("linear", 0.0, 1.0, &mut kl) {
                point_light.kl = kl;
        }

        let mut kq = point_light.kq;
        if ui.slider_config("quadratic", 0.0, 1.0)
                .flags(imgui::SliderFlags::LOGARITHMIC)
                .build(&mut kq)
        {
                point_light.kq = kq;
        }
}

pub fn spotlight_mut<T>(ui: &imgui::Ui, spotlight: &mut T)
where
        T: Deref<Target = Spotlight> + DerefMut,
{
        if ui.button("disable") {
                spotlight.color = Vec3::splat(0.0);
        }

        let mut color: [f32; 3] = spotlight.color.into();
        if ui.color_edit3("color", &mut color) {
                spotlight.color = Vec3::from_slice(&color);
        }

        let mut angle = spotlight.radius_angle;
        if imgui::AngleSlider::new("cutoff angle")
                .range_degrees(0.0, 90.0)
                .build(ui, &mut angle)
        {
                spotlight.radius_angle = angle;
        }

        let mut inner_circle = spotlight.inner_radius_percentage;
        if ui.slider("inner circle", 0.0, 1.0, &mut inner_circle) {
                spotlight.inner_radius_percentage = inner_circle;
        }

        let mut kc = spotlight.kc;
        if ui.slider("constant", 0.0, 1.0, &mut kc) {
                spotlight.kc = kc;
        }

        let mut kl = spotlight.kl;
        if ui.slider("linear", 0.0, 1.0, &mut kl) {
                spotlight.kl = kl;
        }

        let mut kq = spotlight.kq;
        if ui.slider_config("quadratic", 0.0, 1.0)
                .flags(imgui::SliderFlags::LOGARITHMIC)
                .build(&mut kq)
        {
                spotlight.kq = kq;
        }
}

pub fn material_mut(ui: &imgui::Ui, material: &mut Material) {
        ui.slider("shininess", 0.0f32, 256.0, &mut material.shininess);
        ui.slider("ambient strength", 0.0f32, 1.0, &mut material.ambient_strength);
        ui.slider("specular strength", 0.0f32, 1.0, &mut material.specular_strength);
        ui.slider("diffuse strength", 0.0f32, 1.0, &mut material.diffuse_strength);
}
