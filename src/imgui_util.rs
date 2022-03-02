use std::{
        convert::TryInto,
        ops::{Deref, DerefMut},
};

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

pub fn transform(ui: &imgui::Ui<'_>, mut transform: Transform) -> Option<Transform> {
        let mut changed = false;

        if imgui::Drag::new("translation")
                .speed(0.1)
                .build_array(ui, (&mut transform.translation).into())
        {
                changed = true;
        }

        let (yaw, pitch, roll) = transform.rotation.euler_angles();
        if let Some(xyz) = imgui_vec3(
                &ui,
                "rotation",
                -180.0,
                180.0,
                Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees()),
        ) {
                transform.rotation =
                        EulerAngles::new(xyz.x.to_radians(), xyz.y.to_radians(), xyz.z.to_radians()).to_quat();
                changed = true;
        }

        if let Some(scale) = imgui_vec3(&ui, "scale", 0.0, 5.0, transform.scale) {
                transform.scale = scale;
                changed = true;
        }

        if changed {
                Some(transform)
        } else {
                None
        }
}

pub fn transform_mut<T>(ui: &imgui::Ui<'_>, transform: &mut T)
where
        T: Deref<Target = Transform> + DerefMut,
{
        if let Some(t) = self::transform(&ui, **transform) {
                **transform = t;
        }
}

pub fn euler_angles_mut<T>(ui: &imgui::Ui<'_>, euler_angles: &mut T)
where
        T: Deref<Target = EulerAngles> + DerefMut,
{
        let mut pitch = euler_angles.pitch();
        if imgui::AngleSlider::new("pitch")
                .range_degrees(-90.0, 90.0)
                .build(&ui, &mut pitch)
        {
                euler_angles.set_pitch(pitch);
        }

        let mut yaw = euler_angles.yaw();
        if imgui::AngleSlider::new("yaw")
                .range_degrees(-180.0, 180.0)
                .build(&ui, &mut yaw)
        {
                euler_angles.set_yaw(yaw);
        }

        let mut roll = euler_angles.roll();
        if imgui::AngleSlider::new("roll")
                .range_degrees(-180.0, 180.0)
                .build(&ui, &mut roll)
        {
                euler_angles.set_roll(roll);
        }
}

pub fn dir_light_mut<T>(ui: &imgui::Ui<'_>, dir_light: &mut T)
where
        T: Deref<Target = DirectionalLight> + DerefMut,
{
        if ui.button("disable") {
                dir_light.color = Vec3::from_element(0.0);
        }

        let mut direction = dir_light.direction;
        if imgui::Slider::new("direction", -1.0, 1.0).build_array(&ui, (&mut direction).into()) {
                dir_light.direction = direction;
        }

        let mut color: [f32; 3] = dir_light.color.try_into().unwrap();
        if imgui::ColorEdit::new("color", &mut color).build(&ui) {
                dir_light.color = Vec3::from_column_slice(&color);
        }
}

pub fn point_light_mut<T>(ui: &imgui::Ui<'_>, point_light: &mut T)
where
        T: Deref<Target = PointLight> + DerefMut,
{
        if ui.button("disable") {
                point_light.color = Vec3::from_element(0.0);
        }

        let mut color: [f32; 3] = point_light.color.try_into().unwrap();
        if imgui::ColorEdit::new("color", &mut color).build(&ui) {
                point_light.color = Vec3::from_column_slice(&color);
        }

        let mut kc = point_light.kc;
        if imgui::Slider::new("constant", 0.0, 1.0).build(&ui, &mut kc) {
                point_light.kc = kc;
        }

        let mut kl = point_light.kl;
        if imgui::Slider::new("linear", 0.0, 1.0).build(&ui, &mut kl) {
                point_light.kl = kl;
        }

        let mut kq = point_light.kq;
        if imgui::Slider::new("quadratic", 0.0, 1.0)
                .flags(imgui::SliderFlags::LOGARITHMIC)
                .build(&ui, &mut kq)
        {
                point_light.kq = kq;
        }
}

pub fn spotlight_mut<T>(ui: &imgui::Ui<'_>, spotlight: &mut T)
where
        T: Deref<Target = Spotlight> + DerefMut,
{
        if ui.button("disable") {
                spotlight.color = Vec3::from_element(0.0);
        }

        let mut color: [f32; 3] = spotlight.color.try_into().unwrap();
        if imgui::ColorEdit::new("color", &mut color).build(&ui) {
                spotlight.color = Vec3::from_column_slice(&color);
        }

        let mut angle = spotlight.radius_angle;
        if imgui::AngleSlider::new("cutoff angle")
                .range_degrees(0.0, 90.0)
                .build(&ui, &mut angle)
        {
                spotlight.radius_angle = angle;
        }

        let mut inner_circle = spotlight.inner_radius_percentage;
        if imgui::Slider::new("inner circle", 0.0, 1.0).build(&ui, &mut inner_circle) {
                spotlight.inner_radius_percentage = inner_circle;
        }

        let mut kc = spotlight.kc;
        if imgui::Slider::new("constant", 0.0, 1.0).build(&ui, &mut kc) {
                spotlight.kc = kc;
        }

        let mut kl = spotlight.kl;
        if imgui::Slider::new("linear", 0.0, 1.0).build(&ui, &mut kl) {
                spotlight.kl = kl;
        }

        let mut kq = spotlight.kq;
        if imgui::Slider::new("quadratic", 0.0, 1.0)
                .flags(imgui::SliderFlags::LOGARITHMIC)
                .build(&ui, &mut kq)
        {
                spotlight.kq = kq;
        }
}

pub fn material_mut(ui: &imgui::Ui<'_>, material: &mut Material) {
        imgui::Slider::new("shininess", 0.0f32, 256.0).build(&ui, &mut material.shininess);
        imgui::Slider::new("ambient strength", 0.0f32, 1.0).build(&ui, &mut material.ambient_strength);
        imgui::Slider::new("specular strength", 0.0f32, 1.0).build(&ui, &mut material.specular_strength);
        imgui::Slider::new("diffuse strength", 0.0f32, 1.0).build(&ui, &mut material.diffuse_strength);
}
