use std::{
        convert::TryInto,
        path::Path,
        rc::Rc,
        time::{Duration, Instant},
};

use tps_counter::TPSCounter;

use crate::{
        actions::*,
        application_config::ApplicationConfig,
        asset_manager::*,
        components::{
                ActiveCamera, AngularVelocity, Children, DirectionalLight, Force, GlobalTransform,
                ImguiWantCaptureKeyboard, ImguiWantCaptureMouse, InterpScalar, Mass, OrbitalVelocity, Parent, Player,
                PointLight, PreviousGlobalTransform, ProjectionCamera, Ticktime, Transform, Velocity,
        },
        constants::{FONT_SIZE, PLAYER_MOVEMENT_SPEED, ROTATION_PER_SECOND},
        euler_angles::EulerAngles,
        hashmap::{GetOrInsertDefault, HashMap},
        input_manager::{
                ActionReceiver, InputBindingMap, InputManager, KeyBindingType, KeyCode, KeyState, MouseMotionType,
        },
        model_instance_manager::CreateModelInstanceFromName,
        my_glm::*,
        renderer::Renderer,
        vk::vk_renderer::VkRenderer,
        AnyResult,
};
use bevy_ecs::prelude::*;
#[allow(unused_imports)]
use log::{error, info, trace};
use serde::{Deserialize, Serialize};
use winit::{
        event::{Event, StartCause, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        monitor::VideoMode,
        window::{Fullscreen, Window, WindowBuilder},
};

#[allow(dead_code)]
pub struct Application {
        event_loop: Option<EventLoop<()>>,

        target_ticktime: f32,
        world: World,
        schedule: Schedule,
        render_schedule: Schedule,
        accumulator: f32,
        window: Rc<Window>,
        window_state: WindowState,
        imgui_manager: ImguiManager,
        input_manager: InputManager,
        dispatch_actions: bool,
        action_receiver: ActionReceiver,

        renderer: Box<dyn Renderer>,

        player_camera_enabled: bool,

        tps_counter: TPSCounter,
        frame_begin: Instant,
        delta_time: f32,
}

impl Application {
        pub fn new() -> AnyResult<Self> {
                let config = ApplicationConfig::from_file(Path::new("config.json"))?;

                let event_loop = EventLoop::new();
                let fullscreen_video_mode = event_loop.primary_monitor().unwrap().video_modes().next().unwrap();
                let window = Rc::new(WindowBuilder::new()
                        .with_fullscreen(match config.window_mode {
                                WindowMode::Windowed => None,
                                WindowMode::Borderless => Some(Fullscreen::Borderless(None)),
                                WindowMode::Fullscreen => Some(Fullscreen::Exclusive(fullscreen_video_mode.clone())),
                        })
                        .with_visible(false)
                        .with_always_on_top(false)
                        .with_min_inner_size(winit::dpi::PhysicalSize::<u32> {
                                width: 144,
                                height: 144,
                        })
                        .build(&event_loop)?);
                trace!("Created window");

                let mut imgui_manager = ImguiManager::new(&window);

                imgui_manager.add_callback(move |ui, world| {
                        imgui::Window::new("Hello world")
                                .size([300.0, 100.0], imgui::Condition::FirstUseEver)
                                .build(ui, || {
                                        let tps_counter = world.get_resource::<TPSCounter>().unwrap();

                                        let mouse_pos = ui.io().mouse_pos;

                                        ui.text(format!(
                                                "fps: {:7.2}   {:5.2}ms",
                                                ui.io().framerate,
                                                1000.0 / ui.io().framerate,
                                        ));
                                        ui.text(format!(
                                                "tps: {:7.2}   {:5.2}ms",
                                                tps_counter.tps(),
                                                tps_counter.ticktime().as_secs_f32() * 1000.0,
                                        ));
                                        ui.text(format!("Mouse pos: ({:.1},{:.1})", mouse_pos[0], mouse_pos[1]));
                                        ui.separator();

                                        let mut player = world.entity_mut(world.get_resource::<Player>().unwrap().0);
                                        let mut player_transform = player.get_mut::<Transform>().unwrap();

                                        let pos = &mut player_transform.translation;
                                        if imgui::Slider::new("translation", -2.5, 2.5).build_array(&ui, pos.into()) {
                                                player_transform.translation = *pos;
                                        }

                                        let mut camera =
                                                world.entity_mut(world.get_resource::<ActiveCamera>().unwrap().0);
                                        let mut camera_orien = camera.get_mut::<EulerAngles>().unwrap();

                                        let mut pitch = camera_orien.pitch();
                                        if imgui::AngleSlider::new("pitch")
                                                .range_degrees(-90.0, 90.0)
                                                .build(&ui, &mut pitch)
                                        {
                                                camera_orien.set_pitch(pitch);
                                        }

                                        let mut yaw = camera_orien.yaw();
                                        if imgui::AngleSlider::new("yaw")
                                                .range_degrees(-180.0, 180.0)
                                                .build(&ui, &mut yaw)
                                        {
                                                camera_orien.set_yaw(yaw);
                                        }

                                        let mut roll = camera_orien.roll();
                                        if imgui::AngleSlider::new("roll")
                                                .range_degrees(-180.0, 180.0)
                                                .build(&ui, &mut roll)
                                        {
                                                camera_orien.set_roll(roll);
                                        }

                                        let mut dir_light =
                                                world.query::<&mut DirectionalLight>().iter_mut(world).next().unwrap();

                                        let mut dir_light_direction = dir_light.direction;
                                        if imgui::Slider::new("directional light direction", -1.0, 1.0)
                                                .build_array(&ui, (&mut dir_light_direction).into())
                                        {
                                                dir_light.direction = dir_light_direction;
                                        }

                                        let mut dir_light_color: [f32; 3] = dir_light.color.try_into().unwrap();
                                        if imgui::ColorEdit::new("directional light color", &mut dir_light_color)
                                                .build(&ui)
                                        {
                                                dir_light.color = Vec3::from_column_slice(&dir_light_color);
                                        }

                                        let (mut light_transform, mut point_light) = world
                                                .query::<(&mut Transform, &mut PointLight)>()
                                                .iter_mut(world)
                                                .next()
                                                .unwrap();

                                        let mut light_translation = light_transform.translation;
                                        if imgui::Slider::new("point light translation", -2.5, 2.5)
                                                .build_array(&ui, (&mut light_translation).into())
                                        {
                                                light_transform.translation = light_translation;
                                        }

                                        let mut light_color: [f32; 3] = point_light.color.try_into().unwrap();
                                        if imgui::ColorEdit::new("point light color", &mut light_color).build(&ui) {
                                                point_light.color = Vec3::from_column_slice(&light_color);
                                        }

                                        let mut point_light_kc = point_light.kc;
                                        if imgui::Slider::new("point light constant", 0.0, 1.0)
                                                .build(&ui, &mut point_light_kc)
                                        {
                                                point_light.kc = point_light_kc;
                                        }

                                        let mut point_light_kl = point_light.kl;
                                        if imgui::Slider::new("point light linear", 0.0, 1.0)
                                                .build(&ui, &mut point_light_kl)
                                        {
                                                point_light.kl = point_light_kl;
                                        }

                                        let mut point_light_kq = point_light.kq;
                                        if imgui::Slider::new("point light quadratic", 0.0, 1.0)
                                                .build(&ui, &mut point_light_kq)
                                        {
                                                point_light.kq = point_light_kq;
                                        }

                                        let mut asset_manager = world.get_resource_mut::<AssetManager>().unwrap();

                                        imgui::TreeNode::new("Materials").build(ui, || {
                                                for (material_id, material) in asset_manager.iter_materials_mut() {
                                                        let material_name = format!(
                                                                "{:?} - {}",
                                                                material_id,
                                                                material.name.as_deref().unwrap_or("unknown")
                                                        );

                                                        imgui::TreeNode::new(material_name).build(ui, || {
                                                                imgui::Slider::new("shininess", 0.0f32, 256.0)
                                                                        .build(&ui, &mut material.shininess);
                                                                imgui::Slider::new("ambient strength", 0.0f32, 1.0)
                                                                        .build(&ui, &mut material.ambient_strength);
                                                                imgui::Slider::new("specular strength", 0.0f32, 1.0)
                                                                        .build(&ui, &mut material.specular_strength);
                                                                imgui::Slider::new("diffuse strength", 0.0f32, 1.0)
                                                                        .build(&ui, &mut material.diffuse_strength);
                                                        });
                                                }
                                        });
                                });

                        ui.show_demo_window(&mut false);

                        world.get_resource_mut::<ImguiWantCaptureMouse>().unwrap().0 = ui.io().want_capture_mouse;
                        world.get_resource_mut::<ImguiWantCaptureKeyboard>().unwrap().0 = ui.io().want_capture_keyboard;
                });

                let window_state = WindowState::new(
                        &window,
                        imgui_manager.imgui_context.io_mut(),
                        WindowMode::Windowed,
                        fullscreen_video_mode,
                        CursorState::Normal,
                );
                trace!("Initialized ImGui");

                let mut world = World::default();
                world.insert_resource(Ticktime(1.0 / config.tps as f32));
                world.insert_resource(ImguiWantCaptureMouse(false));
                world.insert_resource(ImguiWantCaptureKeyboard(false));
                world.insert_resource(TPSCounter::new(20));

                let asset_manager = Self::init_asset_manager()?;

                world.insert_resource(asset_manager);
                world.insert_resource(ControlFlow::Poll);
                world.insert_resource(window_state.window_mode);
                world.insert_resource(window_state.cursor_state);
                world.insert_resource(Vec::<WindowCommand>::new());

                let mut startup_schedule = Schedule::default();
                let startup = SystemStage::single_threaded().with_system(spawn_entities);
                startup_schedule.add_stage("startup", startup);
                startup_schedule.run(&mut world);

                let mut schedule = Schedule::default();

                let first_stage = SystemStage::single_threaded()
                        .with_system(update_tps_counter)
                        .with_system(hierarchy_maintenance_system.label("hierarchy-maintenance"))
                        .with_system(renormalize_quaternions.after("hierarchy-maintenance"))
                        .with_system(init_new_transforms.label("init-new-transforms"));
                schedule.add_stage("first", first_stage);

                let update = SystemStage::single_threaded()
                        .with_system(persist_transforms.label("persist-transforms"))
                        .with_system(process_actions.label("process-actions").after("persist-transforms"))
                        .with_system(apply_euler_angles.label("apply-euler-angles").after("process-actions"))
                        .with_system(integrate_force.label("linear-force").after("apply-euler-angles"))
                        .with_system(integrate_linear_velocity.label("linear-velocity").after("linear-force"))
                        .with_system(
                                integrate_angular_velocities
                                        .label("angular-velocity")
                                        .after("linear-velocity"),
                        )
                        .with_system(
                                integrate_orbital_velocities
                                        .label("orbital-velocity")
                                        .after("angular-velocity"),
                        )
                        .with_system(
                                global_transform_system
                                        .label("global-transform-system")
                                        .after("orbital-velocity"),
                        );

                schedule.add_stage("update", update);

                let mut render_schedule = Schedule::default();
                render_schedule.add_stage(
                        "render",
                        SystemStage::single_threaded()
                                .with_system(apply_euler_angles.label("apply-euler-angles"))
                                .with_system(
                                        interpolate_transforms
                                                .label("interpolate-transforms")
                                                .after("apply-euler-angles"),
                                ),
                );

                let renderer = Box::new(VkRenderer::new(Rc::clone(&window), &mut imgui_manager.imgui_context)?);

                let dispatch_actions = true;
                let mut input_manager = InputManager::new(dispatch_actions);

                let mut input_map = InputBindingMap::new();

                input_map.bind_key(EXIT, KeyCode::Escape, KeyBindingType::Simple(KeyState::Released));
                input_map.bind_key(TOGGLE_CURSOR, KeyCode::T, KeyBindingType::Simple(KeyState::Released));
                input_map.bind_key(
                        CYCLE_WINDOW_MODE,
                        KeyCode::F11,
                        KeyBindingType::Simple(KeyState::Released),
                );

                input_map.bind_key(MOVE_FORWARD, KeyCode::W, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_BACKWARD, KeyCode::S, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_RIGHTWARD, KeyCode::D, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_LEFTWARD, KeyCode::A, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_UPWARD, KeyCode::Space, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_DOWNARD, KeyCode::LShift, KeyBindingType::Continuous);

                input_map.bind_key(YAW_NEGATIVE, KeyCode::Numpad6, KeyBindingType::Continuous);
                input_map.bind_key(YAW_POSITIVE, KeyCode::Numpad4, KeyBindingType::Continuous);
                input_map.bind_key(PITCH_POSITIVE, KeyCode::Numpad8, KeyBindingType::Continuous);
                input_map.bind_key(PITCH_NEGATIVE, KeyCode::Numpad5, KeyBindingType::Continuous);
                input_map.bind_key(ROLL_NEGATIVE, KeyCode::Numpad9, KeyBindingType::Continuous);
                input_map.bind_key(ROLL_POSITIVE, KeyCode::Numpad7, KeyBindingType::Continuous);

                input_map.bind_mouse_motion(YAW_NEGATIVE, MouseMotionType::PositiveX, None);
                input_map.bind_mouse_motion(YAW_POSITIVE, MouseMotionType::NegativeX, None);
                input_map.bind_mouse_motion(PITCH_POSITIVE, MouseMotionType::PositiveY, None);
                input_map.bind_mouse_motion(PITCH_NEGATIVE, MouseMotionType::NegativeY, None);

                input_manager.push_input_binding_map(input_map);
                trace!("Initialized InputManager");

                world.insert_non_send(input_manager.create_action_receiver());
                let action_receiver = input_manager.create_action_receiver();

                schedule.run(&mut world);

                Ok(Self {
                        event_loop: Some(event_loop),
                        target_ticktime: 1.0 / config.tps as f32,
                        world,
                        schedule,
                        render_schedule,
                        accumulator: 0.0,
                        window,
                        window_state,
                        imgui_manager,
                        input_manager,
                        dispatch_actions,
                        action_receiver,

                        renderer,

                        player_camera_enabled: false,

                        tps_counter: TPSCounter::new(20),
                        frame_begin: Instant::now(),
                        delta_time: 0.0,
                })
        }

        pub fn run(mut self) -> ! {
                self.window.set_visible(true);

                self.event_loop.take().unwrap().run(move |event, _, control_flow| {
                        *control_flow = ControlFlow::Poll;

                        self.on_winit_event(event, control_flow)
                                .expect("Error ocurred in render loop");
                });
        }

        fn init_asset_manager() -> AnyResult<AssetManager> {
                let mut asset_manager = AssetManager::new()?;

                // asset_manager.register_shader_resource(
                //         "matrices".to_string(),
                //         ShaderResource {
                //                 elements: vec![
                //                         ShaderResourceElement {
                //                                 element_type: ShaderResourceElementType::UniformBuffer,
                //                                 shader_stage_flags: ash::vk::ShaderStageFlags::VERTEX,
                //                         },
                //                         ShaderResourceElement {
                //                                 element_type: ShaderResourceElementType::UniformBufferDynamic,
                //                                 shader_stage_flags: ash::vk::ShaderStageFlags::VERTEX,
                //                         },
                //                 ],
                //         },
                // );

                // asset_manager.register_shader_resource(
                //         "material-texture-sampler".to_string(),
                //         ShaderResource {
                //                 elements: vec![
                //                         ShaderResourceElement {
                //                                 element_type: ShaderResourceElementType::SampledImage,
                //                                 shader_stage_flags: ash::vk::ShaderStageFlags::FRAGMENT,
                //                         },
                //                         ShaderResourceElement {
                //                                 element_type: ShaderResourceElementType::Sampler,
                //                                 shader_stage_flags: ash::vk::ShaderStageFlags::FRAGMENT,
                //                         },
                //                 ],
                //         },
                // );

                let _basic_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/basic_shader/basic_shader.yaml"))?;
                let _color_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/color_shader/color_shader.yaml"))?;

                let _model_colt = asset_manager.import_gltf_file(Path::new("res/model/new-colt/colt.gltf"))?;
                let _model_grass_plane =
                        asset_manager.import_gltf_file(Path::new("res/model/grass-plane/grass-plane.gltf"))?;
                let _model_sphere = asset_manager.import_gltf_file(Path::new("res/model/sphere/sphere.gltf"))?;
                let _model_icosphere =
                        asset_manager.import_gltf_file(Path::new("res/model/icosphere/icosphere.gltf"))?;
                let _model_lit_icosphere = asset_manager
                        .import_gltf_file(std::path::Path::new("res/model/lit-icosphere/lit-icosphere.gltf"))?;
                let _model_backpack =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/backpack/backpack.gltf"))?;

                trace!("Initialized AssetManager");
                Ok(asset_manager)
        }

        fn on_winit_event(
                &mut self,
                event: winit::event::Event<'_, ()>,
                control_flow: &mut ControlFlow,
        ) -> AnyResult<()> {
                self.imgui_manager.handle_winit_event(&self.window, &event);

                match event {
                        Event::NewEvents(start_cause) => {
                                self.on_new_events(start_cause);
                        },
                        Event::DeviceEvent { event, .. } => {
                                self.input_manager.on_device_event(&event);
                        },
                        Event::WindowEvent { window_id, event } if self.window.id() == window_id => {
                                self.on_window_event(event, control_flow);
                        },
                        Event::MainEventsCleared => self.update(control_flow)?,
                        Event::LoopDestroyed => self.on_quit(),
                        _ => (),
                }

                Ok(())
        }

        fn on_new_events(&mut self, _start_cause: StartCause) {
                let previous_frame_begin = std::mem::replace(&mut self.frame_begin, Instant::now());
                let delta_time = self.frame_begin - previous_frame_begin;

                self.delta_time = delta_time.as_secs_f32();
                self.imgui_manager.on_delta_time_updated(delta_time);
        }

        fn on_window_event(&mut self, window_event: WindowEvent, control_flow: &mut ControlFlow) {
                match window_event {
                        /* WindowEvent::ModifiersChanged(modifiers_state) => {
                                self.input_manager.on_modifiers_changed(modifiers_state)
                        } */
                        WindowEvent::Focused(focused) => {
                                self.window_state.on_window_focused(focused, &self.window);
                                self.input_manager.set_dispatch_actions(self.should_dispatch_actions());
                        },
                        WindowEvent::Resized(new_size) => {
                                self.renderer.on_window_resize(new_size.width, new_size.height);
                        },
                        WindowEvent::CloseRequested => {
                                *control_flow = ControlFlow::Exit;
                        },
                        _ => {},
                };
        }

        fn update(&mut self, control_flow: &mut ControlFlow) -> AnyResult<()> {
                self.input_manager.set_dispatch_actions(self.should_dispatch_actions());

                let mut camera = self
                        .world
                        .entity_mut(self.world.get_resource::<ActiveCamera>().unwrap().0);
                let mut camera_orien = camera.get_mut::<EulerAngles>().unwrap();

                for (action_id, strength) in self.action_receiver.receive(self.delta_time) {
                        if self.window_state.cursor_state != CursorState::Hidden {
                                continue;
                        }

                        match action_id {
                                YAW_POSITIVE => camera_orien.yaw_by(strength.0 * ROTATION_PER_SECOND),
                                YAW_NEGATIVE => camera_orien.yaw_by(-strength.0 * ROTATION_PER_SECOND),
                                PITCH_POSITIVE => camera_orien.pitch_by(strength.0 * ROTATION_PER_SECOND),
                                PITCH_NEGATIVE => camera_orien.pitch_by(-strength.0 * ROTATION_PER_SECOND),
                                ROLL_POSITIVE => camera_orien.roll_by(strength.0 * ROTATION_PER_SECOND),
                                ROLL_NEGATIVE => camera_orien.roll_by(-strength.0 * ROTATION_PER_SECOND),
                                _ => continue,
                        }
                }

                self.accumulator += self.delta_time;
                if self.accumulator >= self.target_ticktime {
                        self.schedule.run(&mut self.world);
                        self.accumulator -= self.target_ticktime;
                        *control_flow = *self.world.get_resource::<ControlFlow>().unwrap();

                        for window_command in self.world.get_resource_mut::<Vec<WindowCommand>>().unwrap().drain(..) {
                                match window_command {
                                        WindowCommand::SetCursorState(cursor_state) => {
                                                self.window_state.set_cursor_state(
                                                        &self.window,
                                                        self.imgui_manager.imgui_context.io_mut(),
                                                        cursor_state,
                                                );
                                                // input_manager.set_dispatch_actions(new_cursor_state == CursorState::Hidden);
                                        },
                                        WindowCommand::SetWindowMode(window_mode) => {
                                                self.window_state.set_window_mode(&self.window, window_mode);
                                        },
                                }
                        }

                        // self.tps_counter.tick_and_map(|t| info!("TPS: {}", t));
                }

                // self.tps_counter.tick_and_map(|t| info!("TPS: {}", t));
                self.world
                        .insert_resource(InterpScalar(self.accumulator / self.target_ticktime));
                self.render_schedule.run(&mut self.world);
                let ui = self.imgui_manager.build_imgui_ui(&self.window, &mut self.world)?;
                self.renderer.draw_world(&mut self.world, ui.render())?;

                // if let Some(render_state) = self.render_state_manager.get_render_state(self.target_ticktime) {
                //         self.transform_manager.on_update();
                //         self.update_instance_transforms(&render_state);

                //         let imgui_ui = Self::build_imgui_ui(
                //                 &mut self.imgui_context,
                //                 &self.window,
                //                 &mut self.dispatch_actions,
                //                 &mut self.player_orien,
                //                 &mut self.player_transform,
                //         )?;

                //         // self.renderer.draw(
                //         //         &self.mesh_instances,
                //         //         &self.model_instances,
                //         //         &self.model_instances_index,
                //         //         &self.transform_manager,
                //         //         &render_state,
                //         //         &self.player_orien.to_quat(),
                //         //         imgui_ui.render(),
                //         // )?;
                // }

                Ok(())
        }

        fn should_dispatch_actions(&self) -> bool {
                return !(self.world.get_resource::<ImguiWantCaptureMouse>().unwrap().0
                        || self.world.get_resource::<ImguiWantCaptureKeyboard>().unwrap().0
                        || !self.window_state.has_focus);
        }

        fn on_quit(&mut self) {
                self.renderer.destroy().unwrap();
        }
}

pub struct WindowState {
        window_mode: WindowMode,
        fullscreen_video_mode: VideoMode,
        cursor_state: CursorState,
        has_focus: bool,
}

impl WindowState {
        fn new(
                window: &Window,
                imgui_io: &mut imgui::Io,
                window_mode: WindowMode,
                fullscreen_video_mode: VideoMode,
                cursor_state: CursorState,
        ) -> Self {
                Self::set_cursor_state_inner(window, imgui_io, cursor_state);

                Self {
                        window_mode,
                        fullscreen_video_mode,
                        cursor_state,
                        has_focus: true,
                }
        }

        pub fn on_window_focused(&mut self, focused: bool, window: &Window) {
                self.has_focus = focused;

                if self.window_mode == WindowMode::Fullscreen {
                        let window_mode = if focused {
                                WindowMode::Fullscreen
                        } else {
                                WindowMode::Windowed
                        };

                        self.set_window_mode_silently(&window, window_mode);
                }
        }

        pub fn set_cursor_state(&mut self, window: &Window, imgui_io: &mut imgui::Io, cursor_state: CursorState) {
                self.cursor_state = cursor_state;
                Self::set_cursor_state_inner(window, imgui_io, cursor_state);
        }

        pub fn set_window_mode(&mut self, window: &Window, window_mode: WindowMode) {
                self.window_mode = window_mode;
                self.set_window_mode_silently(window, window_mode);
        }

        pub fn set_window_mode_silently(&mut self, window: &Window, window_mode: WindowMode) {
                window.set_fullscreen(match window_mode {
                        WindowMode::Windowed => None,
                        WindowMode::Borderless => Some(Fullscreen::Borderless(None)),
                        WindowMode::Fullscreen => Some(Fullscreen::Exclusive(self.fullscreen_video_mode.clone())),
                });
        }

        fn set_cursor_state_inner(window: &Window, imgui_io: &mut imgui::Io, cursor_state: CursorState) {
                window.set_cursor_visible(cursor_state != CursorState::Hidden);
                window.set_cursor_grab(cursor_state == CursorState::Hidden).unwrap();
                imgui_io.config_flags
                        .set(imgui::ConfigFlags::NO_MOUSE, cursor_state == CursorState::Hidden);
        }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorState {
        Normal,
        Hidden,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
        Windowed,
        Borderless,
        Fullscreen,
}

fn spawn_entities(mut commands: Commands) {
        let player = commands
                .spawn()
                .insert(Transform::from_translation(Vec3::new(0.0, 0.0, 2.0)))
                .id();
        commands.insert_resource(Player(player));

        let player_head = commands
                .spawn()
                .insert(Parent(player))
                .insert(Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)))
                .insert(EulerAngles::new(0.0, 0.0, 0.0))
                .insert(ProjectionCamera::new(90.0f32.to_radians(), 1.0, 0.1, 100.0))
                .id();
        commands.insert_resource(ActiveCamera(player_head));

        let colt = commands
                .spawn()
                .insert(Transform::from_translation(Vec3::new(2.5, 0.0, 0.0)))
                .insert(AngularVelocity(Vec3::y() * 45.0f32.to_radians()))
                .insert(OrbitalVelocity {
                        origin: Vec3::new(0.0, 2.5, 0.0),
                        velocity: (Vec3::x() + Vec3::y()).normalize() * -22.5f32.to_radians(),
                })
                .id();

        commands.add(CreateModelInstanceFromName {
                entity: colt,
                model_name: "colt".to_string(),
        });

        let icosphere = commands
                .spawn()
                .insert(Transform::from_scale(Vec3::new(4.0, 4.0, 4.0)))
                .insert(Force(Vec3::new(0.0, 0.0, 0.0)))
                .insert(Mass(1.0))
                .insert(Velocity(Vec3::new(0.0, 0.0, 0.0)))
                .id();

        commands.add(CreateModelInstanceFromName {
                entity: icosphere,
                model_name: "icosphere".to_string(),
        });

        let backpack = commands
                .spawn()
                .insert(Transform {
                        translation: Vec3::new(2.0, 0.0, 0.0),
                        scale: Vec3::from_element(5.0),
                        ..Transform::identity()
                })
                .insert(AngularVelocity(Vec3::y() * 22.5f32.to_radians()))
                .insert(OrbitalVelocity {
                        origin: Vec3::new(0.0, 0.0, 0.0),
                        velocity: Vec3::y() * -22.5f32.to_radians(),
                })
                .id();

        commands.add(CreateModelInstanceFromName {
                entity: backpack,
                model_name: "backpack".to_string(),
        });

        let grass_plane = commands
                .spawn()
                .insert(Transform::from_translation(Vec3::new(0.0, -1.0, 0.0)))
                .id();

        commands.add(CreateModelInstanceFromName {
                entity: grass_plane,
                model_name: "grass-plane".to_string(),
        });

        let light = commands
                .spawn()
                .insert(Transform {
                        translation: Vec3::new(1.0, 2.0, 0.0),
                        rotation: UnitQuat::identity(),
                        scale: Vec3::from_element(0.25),
                })
                .insert(PointLight {
                        color: Vec3::new(0.9, 1.0, 0.9),
                        kc: 1.0,
                        kl: 0.0,
                        kq: 1.0,
                })
                .insert(OrbitalVelocity {
                        origin: Vec3::from_element(0.0),
                        velocity: Vec3::y() * 45.0f32.to_radians(),
                })
                .id();

        commands.add(CreateModelInstanceFromName {
                entity: light,
                model_name: "lit-icosphere".to_string(),
        });

        let _dir_light = commands
                .spawn()
                .insert(DirectionalLight {
                        direction: Vec3::new(1.0, -1.0, 0.0),
                        color: Vec3::new(0.9, 1.0, 0.9),
                })
                .id();
}

fn process_actions(
        mut commands: Commands,
        ticktime: Res<Ticktime>,
        player: Res<Player>,
        camera: Res<ActiveCamera>,
        action_receiver: NonSend<ActionReceiver>,
        mut control_flow: ResMut<ControlFlow>,
        mut window_mode: ResMut<WindowMode>,
        mut cursor_state: ResMut<CursorState>,
        // TODO: make commands by observing modifications to Res<WindowMode>, etc.
        mut window_commands: ResMut<Vec<WindowCommand>>,
        mut transforms: Query<&mut Transform>,
) {
        let mut desired_dir = Vec3::new(0.0, 0.0, 0.0);

        for (action_id, strength) in action_receiver.receive(ticktime.0) {
                match action_id {
                        MOVE_FORWARD => desired_dir.z -= strength.0,
                        MOVE_BACKWARD => desired_dir.z += strength.0,
                        MOVE_RIGHTWARD => desired_dir.x += strength.0,
                        MOVE_LEFTWARD => desired_dir.x -= strength.0,
                        MOVE_UPWARD => desired_dir.y += strength.0,
                        MOVE_DOWNARD => desired_dir.y -= strength.0,
                        EXIT => {
                                *control_flow = ControlFlow::Exit;
                        },
                        TOGGLE_CURSOR => {
                                *cursor_state = match *cursor_state {
                                        CursorState::Normal => CursorState::Hidden,
                                        CursorState::Hidden => CursorState::Normal,
                                };

                                window_commands.push(WindowCommand::SetCursorState(*cursor_state));
                        },
                        CYCLE_WINDOW_MODE => {
                                *window_mode = match *window_mode {
                                        WindowMode::Windowed => WindowMode::Borderless,
                                        WindowMode::Borderless => WindowMode::Fullscreen,
                                        WindowMode::Fullscreen => WindowMode::Windowed,
                                };

                                window_commands.push(WindowCommand::SetWindowMode(*window_mode));
                        },
                        _ => (),
                }
        }

        if desired_dir.norm_squared() > f32::EPSILON {
                let camera_orien = &transforms.get(camera.0).unwrap().rotation;
                let camera_hor_orien = UnitQuat::new_normalize(Quat::new(
                        camera_orien.as_vector().w,
                        0.0,
                        camera_orien.as_vector().y,
                        0.0,
                ));

                let move_vector = camera_hor_orien * desired_dir * PLAYER_MOVEMENT_SPEED;

                let mut player_transform = transforms.get_mut(player.0).unwrap();
                player_transform.translation += move_vector;
        }
}

fn hierarchy_maintenance_system(
        mut commands: Commands,
        new_children_query: Query<(Entity, &Parent), Added<Parent>>,
        mut children_query: Query<&mut Children>,
) {
        let mut new_parents: HashMap<Entity, Vec<Entity>> = HashMap::new();

        for (new_child, parent) in new_children_query.iter() {
                if let Ok(mut parents_children) = children_query.get_mut(parent.0) {
                        parents_children.0.push(new_child);
                } else {
                        new_parents.get_mut_or_insert_default(&parent.0).push(new_child);
                }
        }

        for (new_parent, children) in new_parents {
                commands.entity(new_parent).insert(Children(children));
        }
}

fn global_transform_system(
        mut root_query: Query<(Entity, &Transform, &mut GlobalTransform, Option<&Children>), Without<Parent>>,
        mut transform_query: Query<(&Transform, &mut GlobalTransform), With<Parent>>,
        changed_transform_query: Query<Entity, Changed<Transform>>,
        children_query: Query<&Children>,
) {
        for (root, root_transform, mut root_global_transform, children) in root_query.iter_mut() {
                let transform_changed = changed_transform_query.get(root).is_ok();
                if transform_changed {
                        root_global_transform.0 = *root_transform;
                };

                if let Some(children) = children {
                        for &child in &children.0 {
                                update_global_transform_recursive(
                                        &mut transform_query,
                                        &changed_transform_query,
                                        &children_query,
                                        &root_global_transform,
                                        transform_changed,
                                        child,
                                );
                        }
                }
        }
}

fn update_global_transform_recursive(
        transform_query: &mut Query<(&Transform, &mut GlobalTransform), With<Parent>>,
        changed_transform_query: &Query<Entity, Changed<Transform>>,
        children_query: &Query<&Children>,
        parent_transform: &GlobalTransform,
        parent_transform_changed: bool,
        entity: Entity,
) {
        let (transform, mut global_transform) = match transform_query.get_mut(entity) {
                Ok(t) => t,
                Err(_) => return,
        };

        let should_update_global_transform = parent_transform_changed || changed_transform_query.get(entity).is_ok();
        if should_update_global_transform {
                global_transform.0 = parent_transform.0 * *transform;
        }

        let global_transform = *global_transform;

        if let Ok(children) = children_query.get(entity) {
                for &child in &children.0 {
                        update_global_transform_recursive(
                                transform_query,
                                changed_transform_query,
                                children_query,
                                &global_transform,
                                should_update_global_transform,
                                child,
                        );
                }
        }
}

fn init_new_transforms(mut commands: Commands, new_transforms: Query<(Entity, &Transform), Added<Transform>>) {
        for (e, new_transform) in new_transforms.iter() {
                commands.entity(e).insert(GlobalTransform(*new_transform));
                commands.entity(e).insert(PreviousGlobalTransform(*new_transform));
                commands.entity(e).insert(InterpGlobalTransform(*new_transform));
        }
}

fn update_tps_counter(mut tps_counter: ResMut<TPSCounter>) {
        tps_counter.tick();
}

fn renormalize_quaternions(mut transforms: Query<&mut Transform, Changed<Transform>>) {
        for mut transform in transforms.iter_mut() {
                transform.rotation = UnitQuat::new_normalize(*transform.rotation);
        }
}

fn persist_transforms(mut transforms: Query<(&GlobalTransform, &mut PreviousGlobalTransform)>) {
        for (transform, mut old_transform) in transforms.iter_mut() {
                old_transform.0 = transform.0;
        }
}

fn apply_euler_angles(mut query: Query<(&EulerAngles, &mut Transform)>) {
        for (euler_angles, mut transform) in query.iter_mut() {
                transform.rotation = euler_angles.to_quat();
        }
}

fn integrate_force(mut query: Query<(&Force, &Mass, &mut Velocity)>, ticktime: Res<Ticktime>) {
        for (force, mass, mut velocity) in query.iter_mut() {
                let momentum = force.0 * ticktime.0;
                let delta_velocity = momentum / mass.0;
                velocity.0 += delta_velocity;
        }
        // for (force, mass, mut velocity) in query.iter_mut() {
        //         let acceleration = force.0 / mass.0;
        //         velocity.0 += acceleration * ticktime.0;
        // }
}

fn integrate_linear_velocity(mut query: Query<(&Velocity, &mut Transform)>, ticktime: Res<Ticktime>) {
        for (velocity, mut transform) in query.iter_mut() {
                transform.translation += velocity.0 * ticktime.0;
        }
}

fn integrate_angular_velocities(mut query: Query<(&AngularVelocity, &mut Transform)>, ticktime: Res<Ticktime>) {
        for (angular_velocity, mut transform) in query.iter_mut() {
                transform.rotation *= UnitQuat::new(angular_velocity.0 * ticktime.0);
        }
}

fn integrate_orbital_velocities(mut query: Query<(&OrbitalVelocity, &mut Transform)>, ticktime: Res<Ticktime>) {
        for (orbital_velocity, mut transform) in query.iter_mut() {
                let orbital_pos = transform.translation - orbital_velocity.origin;
                let orbital_rot = UnitQuat::new(orbital_velocity.velocity * ticktime.0);
                let new_orbital_pos = orbital_rot * orbital_pos;
                let delta_pos = new_orbital_pos - orbital_pos;

                transform.translation += delta_pos;
        }
}

fn interpolate_transforms(
        mut query: Query<(&PreviousGlobalTransform, &GlobalTransform, &mut InterpGlobalTransform)>,
        t: Res<InterpScalar>,
) {
        for (prev_transform, curr_transform, mut interp_transform) in query.iter_mut() {
                interp_transform.0 = Transform::interp(&prev_transform.0, &curr_transform.0, t.0);
        }
}

pub struct ImguiManager {
        imgui_context: imgui::Context,
        imgui_platform: imgui_winit_support::WinitPlatform,
        callbacks: Vec<Box<dyn FnMut(&mut imgui::Ui<'_>, &mut World)>>,
}

impl ImguiManager {
        fn new(window: &Window) -> Self {
                let mut imgui_context = imgui::Context::create();
                let mut imgui_platform = imgui_winit_support::WinitPlatform::init(&mut imgui_context);

                let hidpi_factor = imgui_platform.hidpi_factor() as f32;
                let font_size = FONT_SIZE * hidpi_factor;
                imgui_context.fonts().add_font(&[
                        imgui::FontSource::DefaultFontData {
                                config: Some(imgui::FontConfig {
                                        size_pixels: font_size,
                                        ..imgui::FontConfig::default()
                                }),
                        },
                        imgui::FontSource::TtfData {
                                data: include_bytes!("../res/font/FiraCode-Regular.ttf"),
                                size_pixels: font_size,
                                config: Some(imgui::FontConfig {
                                        rasterizer_multiply: 1.75,
                                        glyph_ranges: imgui::FontGlyphRanges::japanese(),
                                        ..imgui::FontConfig::default()
                                }),
                        },
                ]);
                imgui_context.io_mut().font_global_scale = 1.0 / hidpi_factor;

                imgui_platform.attach_window(imgui_context.io_mut(), &window, imgui_winit_support::HiDpiMode::Rounded);

                Self {
                        imgui_context,
                        imgui_platform,
                        callbacks: Vec::new(),
                }
        }

        fn add_callback<T: FnMut(&mut imgui::Ui<'_>, &mut World) + 'static>(&mut self, f: T) {
                self.callbacks.push(Box::new(f));
        }

        fn handle_winit_event(&mut self, window: &Window, event: &winit::event::Event<'_, ()>) {
                self.imgui_platform
                        .handle_event(self.imgui_context.io_mut(), window, event);
        }

        fn on_delta_time_updated(&mut self, delta_time: Duration) {
                self.imgui_context.io_mut().update_delta_time(delta_time);
        }

        fn build_imgui_ui<'a>(
                &'a mut self,
                window: &winit::window::Window,
                world: &mut World,
        ) -> Result<imgui::Ui<'a>, winit::error::ExternalError> {
                self.imgui_platform.prepare_frame(self.imgui_context.io_mut(), window)?;

                let mut ui = self.imgui_context.frame();

                self.callbacks.iter_mut().for_each(|c| c(&mut ui, world));

                Ok(ui)
        }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct InterpGlobalTransform(pub Transform);

enum WindowCommand {
        SetCursorState(CursorState),
        SetWindowMode(WindowMode),
}
