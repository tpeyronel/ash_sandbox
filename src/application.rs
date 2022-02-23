use std::{
        path::Path,
        rc::Rc,
        sync::{Arc, Mutex},
        time::Instant,
};

use slotmap::SlotMap;
use specs::{ReadExpect, WriteStorage};
use tps_counter::TPSCounter;

use crate::{
        actions::*,
        application_config::ApplicationConfig,
        asset_manager::*,
        components::{
                ActiveCamera, DeltaTime, LightEmitter, OldTransform, OrbitalVelocity, Parent, Player, ProjectionCamera,
                RelativeTransform, Transform,
        },
        constants::{FONT_SIZE, MAX_CONCURRENT_FRAMES, PLAYER_MOVEMENT_SPEED, ROTATION_PER_SECOND},
        euler_angles::EulerAngles,
        input_manager::{
                ActionReceiver, InputBindingMap, InputManager, KeyBindingType, KeyCode, KeyState, MouseMotionType,
        },
        logic_thread::{PlayerResource, TransformComponent},
        model_instance_manager::{ModelInstance, ModelInstanceManager, TransformManager},
        my_glm::*,
        render_state_switcher::RenderStateSwitcher,
        renderer::{ModelInstanceId, RenderState, Renderer},
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
        accumulator: f32,
        window: Rc<Window>,
        window_state: WindowState,
        imgui_context: ImguiContext,
        input_manager: InputManager,
        dispatch_actions: bool,
        action_receiver: ActionReceiver,

        renderer: VkRenderer,

        player_orien: EulerAngles,
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

                let mut imgui_context = Self::init_imgui(&window);

                let window_state = WindowState::new(
                        &window,
                        imgui_context.context.io_mut(),
                        WindowMode::Windowed,
                        fullscreen_video_mode,
                        CursorState::Normal,
                );
                trace!("Initialized ImGui");

                let mut world = World::default();
                world.insert_resource(DeltaTime(1.0 / config.tps as f32));

                let asset_manager = Self::init_asset_manager()?;
                let mut transform_manager = TransformManager::new(MAX_CONCURRENT_FRAMES);
                let model_instance_manager = ModelInstanceManager::new();

                let player = world.spawn().insert(Transform::from_pos(Vec3::new(0.0, 0.0, 2.0))).id();
                world.insert_resource(Player(player));

                let camera = world
                        .spawn()
                        .insert(Parent(player))
                        .insert(Transform::default())
                        .insert(RelativeTransform(Transform::from_pos(Vec3::new(0.0, 1.0, 0.0))))
                        .insert(ProjectionCamera::new(90.0f32.to_radians(), 1.0, 0.1, 100.0))
                        .id();
                world.insert_resource(ActiveCamera(camera));

                let _colt = world
                        .spawn()
                        .insert(Transform::from_pos(Vec3::new(0.0, 0.0, -2.5)))
                        .insert(model_instance_manager.create_model_instance(
                                &asset_manager,
                                &mut transform_manager,
                                asset_manager.get_model_by_name("colt"),
                        ))
                        .insert(OrbitalVelocity {
                                origin: Vec3::from_element(0.0),
                                velocity: Vec3::new(0.0, -22.5f32.to_radians(), 0.0),
                        })
                        .id();

                let _icosphere = world
                        .spawn()
                        .insert(Transform::from_scale(Vec3::new(4.0, 4.0, 4.0)))
                        .insert(model_instance_manager.create_model_instance(
                                &asset_manager,
                                &mut transform_manager,
                                asset_manager.get_model_by_name("icosphere"),
                        ))
                        .insert(OrbitalVelocity {
                                origin: Vec3::from_element(0.0),
                                velocity: Vec3::new(0.0, 0f32.to_radians(), 0.0),
                        })
                        .id();

                let _grass_plane = world
                        .spawn()
                        .insert(Transform::from_pos(Vec3::new(0.0, -1.0, 0.0)))
                        .insert(model_instance_manager.create_model_instance(
                                &asset_manager,
                                &mut transform_manager,
                                asset_manager.get_model_by_name("grass-plane"),
                        ))
                        .id();

                let _light = world
                        .spawn()
                        .insert(Transform {
                                pos: Vec3::new(1.0, 2.0, 0.0),
                                orien: UnitQuat::identity(),
                                scale: Vec3::from_element(0.25),
                        })
                        .insert(model_instance_manager.create_model_instance(
                                &asset_manager,
                                &mut transform_manager,
                                asset_manager.get_model_by_name("lit-icosphere"),
                        ))
                        .insert(LightEmitter {
                                color: Vec3::new(0.9, 1.0, 0.9),
                        })
                        .insert(OrbitalVelocity {
                                origin: Vec3::from_element(0.0),
                                velocity: Vec3::new(0.0, 45f32.to_radians(), 0.0),
                        })
                        .id();

                world.insert_resource(asset_manager);
                world.insert_resource(transform_manager);
                world.insert_resource(ControlFlow::Poll);
                world.insert_resource(window_state.window_mode);
                world.insert_resource(window_state.cursor_state);
                world.insert_resource(Vec::<WindowCommand>::new());

                let mut schedule = Schedule::default();

                let first_stage = SystemStage::single_threaded().with_system(persist_transforms);
                schedule.add_stage("first", first_stage);

                let update = SystemStage::single_threaded()
                        .with_system(init_new_transforms)
                        .with_system(process_actions)
                        .with_system(relative_transform_updater)
                        .with_system(update_transforms);

                schedule.add_stage("update", update);

                let renderer = VkRenderer::new(Rc::clone(&window), &mut imgui_context.context)?;

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
                        accumulator: 0.0,
                        window,
                        window_state,
                        imgui_context,
                        input_manager,
                        dispatch_actions,
                        action_receiver,

                        renderer,

                        player_orien: EulerAngles::new(0.0, 0.0, 0.0),
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

        fn init_imgui(window: &Window) -> ImguiContext {
                let mut context = imgui::Context::create();
                let mut platform = imgui_winit_support::WinitPlatform::init(&mut context);

                let hidpi_factor = platform.hidpi_factor() as f32;
                let font_size = FONT_SIZE * hidpi_factor;
                context.fonts().add_font(&[
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
                context.io_mut().font_global_scale = 1.0 / hidpi_factor;
                platform.attach_window(context.io_mut(), &window, imgui_winit_support::HiDpiMode::Rounded);

                ImguiContext { context, platform }
        }

        fn init_asset_manager() -> AnyResult<AssetManager> {
                let dsampler = Sampler {
                        name: Some(String::from("Default Sampler")),
                        mag_filter: MagFilter::Linear,
                        min_filter: MinFilter::LinearMipmapLinear,
                        wrap_s: WrappingMode::Repeat,
                        wrap_t: WrappingMode::Repeat,
                };

                // TODO: improve default shader.
                let dmaterial = Material {
                        name: Some(String::from("Default Material")),
                        shader: ShaderId::from(slotmap::KeyData::default()),
                        base_color_factor: Vec4::new(0.8, 0.8, 0.8, 1.0),
                        metallic_factor: 0.0,
                        roughness_factor: 1.0,
                        base_color_texture: None,
                        metallic_roughness_texture: None,
                        normal_texture: None,
                        occlusion_texture: None,
                        emissive_texture: None,
                        emissive_factor: Vec3::from_element(0.0),
                };

                let mut asset_manager = AssetManager::new(dsampler, dmaterial);

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

                trace!("Initialized AssetManager");
                Ok(asset_manager)
        }

        fn on_winit_event(
                &mut self,
                event: winit::event::Event<'_, ()>,
                control_flow: &mut ControlFlow,
        ) -> AnyResult<()> {
                self.imgui_context
                        .platform
                        .handle_event(self.imgui_context.context.io_mut(), &self.window, &event);

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
                self.imgui_context.context.io_mut().update_delta_time(delta_time);
        }

        fn on_window_event(&mut self, window_event: WindowEvent, control_flow: &mut ControlFlow) {
                match window_event {
                        /* WindowEvent::ModifiersChanged(modifiers_state) => {
                                self.input_manager.on_modifiers_changed(modifiers_state)
                        } */
                        WindowEvent::Focused(focused) => {
                                self.window_state.on_window_focused(focused, &self.window);
                                self.input_manager
                                        .set_dispatch_actions(focused && self.dispatch_actions);
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
                self.input_manager
                        .set_dispatch_actions(self.window_state.has_focus && self.dispatch_actions);

                for (action_id, strength) in self.action_receiver.receive_adjusted(self.delta_time) {
                        if self.window_state.cursor_state != CursorState::Hidden {
                                continue;
                        }

                        match action_id {
                                YAW_POSITIVE => self.player_orien.yaw_by(strength.0 * ROTATION_PER_SECOND),
                                YAW_NEGATIVE => self.player_orien.yaw_by(-strength.0 * ROTATION_PER_SECOND),
                                PITCH_POSITIVE => self.player_orien.pitch_by(strength.0 * ROTATION_PER_SECOND),
                                PITCH_NEGATIVE => self.player_orien.pitch_by(-strength.0 * ROTATION_PER_SECOND),
                                ROLL_POSITIVE => self.player_orien.roll_by(strength.0 * ROTATION_PER_SECOND),
                                ROLL_NEGATIVE => self.player_orien.roll_by(-strength.0 * ROTATION_PER_SECOND),
                                _ => continue,
                        }
                }

                let player = self.world.get_resource::<Player>().unwrap().0;
                let mut player_transform = self.world.entity_mut(player).get_mut::<Transform>().unwrap();
                player_transform.orien = self.player_orien.to_quat();

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
                                                        self.imgui_context.context.io_mut(),
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
                interpolate_transforms(&mut self.world, self.accumulator / self.target_ticktime);
                self.renderer.draw_world(&mut self.world)?;

                // self.logic_thread_tx
                //         .send(LogicThreadMessage::SetPlayerOrien(self.player_orien.to_quat()))
                //         .expect("Failed to send command to logic thread!");

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

        fn process_model_instance(
                matrix_stack: &mut MatrixStack,
                transform_manager: &mut TransformManager,
                asset_manager: &AssetManager,
                model_instances: &SlotMap<ModelInstanceId, crate::renderer::ModelInstance>,
                model_instance: &crate::renderer::ModelInstance,
        ) {
                matrix_stack.push(model_instance.transform.to_matrix());
                let transform = matrix_stack.push(asset_manager.models()[model_instance.model_id].base_transform);

                // for &mesh_instance_id in &model_instance.mesh_instances {
                //         transform_manager.set_transform(mesh_instance_id, &transform);
                // }

                for &child_model_instance_id in &model_instance.children {
                        Self::process_model_instance(
                                matrix_stack,
                                transform_manager,
                                asset_manager,
                                model_instances,
                                &model_instances[child_model_instance_id],
                        );
                }

                matrix_stack.pop();
                matrix_stack.pop();
        }

        fn build_imgui_ui<'a>(
                imgui_context: &'a mut ImguiContext,
                window: &winit::window::Window,
                dispatch_actions: &mut bool,
                player_orien: &mut EulerAngles,
                player_transform: &mut SharedValueSlave<TransformComponent>,
        ) -> Result<imgui::Ui<'a>, winit::error::ExternalError> {
                imgui_context
                        .platform
                        .prepare_frame(imgui_context.context.io_mut(), window)?;

                let ui = imgui_context.context.frame();

                imgui::Window::new("Hello world")
                        .size([300.0, 100.0], imgui::Condition::FirstUseEver)
                        .build(&ui, || {
                                let mouse_pos = ui.io().mouse_pos;

                                ui.text(format!(
                                        "fps: {:7.2}   {:5.2}ms",
                                        ui.io().framerate,
                                        1000.0 / ui.io().framerate
                                ));
                                ui.text(format!("Mouse pos: ({:.1},{:.1})", mouse_pos[0], mouse_pos[1]));
                                ui.separator();
                                ui.text(format!(
                                        "Pitch: {:.1}, Yaw: {:.1}, Roll: {:.1}",
                                        player_orien.pitch().to_degrees(),
                                        player_orien.yaw().to_degrees(),
                                        player_orien.roll().to_degrees(),
                                ));

                                let pos = &mut player_transform.get_mut().pos;
                                if imgui::Slider::new("position", -2.5, 2.5).build_array(&ui, pos.into()) {
                                        player_transform.reemit();
                                }

                                let mut pitch = player_orien.pitch();
                                if imgui::AngleSlider::new("pitch")
                                        .range_degrees(-90.0, 90.0)
                                        .build(&ui, &mut pitch)
                                {
                                        player_orien.set_pitch(pitch);
                                }

                                let mut yaw = player_orien.yaw();
                                if imgui::AngleSlider::new("yaw")
                                        .range_degrees(-180.0, 180.0)
                                        .build(&ui, &mut yaw)
                                {
                                        player_orien.set_yaw(yaw);
                                }

                                let mut roll = player_orien.roll();
                                if imgui::AngleSlider::new("roll")
                                        .range_degrees(-180.0, 180.0)
                                        .build(&ui, &mut roll)
                                {
                                        player_orien.set_roll(roll);
                                }
                        });

                ui.show_demo_window(&mut false);

                *dispatch_actions = !ui.io().want_capture_mouse;

                Ok(ui)
        }

        // fn process_logic_thread_messages(&mut self, control_flow: &mut ControlFlow) {
        //         for message in self.window_thread_rx.try_iter() {
        //                 match message {
        //                         WindowThreadMessage::Command(command) => Self::process_command(
        //                                 command,
        //                                 control_flow,
        //                                 &self.window,
        //                                 &mut self.imgui_context.context.io_mut(),
        //                                 &mut self.window_state,
        //                                 &mut self.player_camera_enabled,
        //                         ),
        //                 }
        //         }
        // }

        fn process_command(
                command: WindowThreadCommand,
                control_flow: &mut ControlFlow,
                window: &winit::window::Window,
                imgui_io: &mut imgui::Io,
                window_state: &mut WindowState,
                player_camera_enabled: &mut bool,
        ) {
                match command {
                        WindowThreadCommand::Exit => *control_flow = ControlFlow::Exit,
                        WindowThreadCommand::SetCursorState(cursor_state) => {
                                window_state.set_cursor_state(window, imgui_io, cursor_state);
                        },
                        WindowThreadCommand::SetWindowMode(window_mode) => {
                                window_state.set_window_mode(window, window_mode);
                        },
                        WindowThreadCommand::SetPlayerCameraEnabled(enabled) => {
                                *player_camera_enabled = enabled;
                        },
                }
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

struct ImguiContext {
        context: imgui::Context,
        platform: imgui_winit_support::WinitPlatform,
}

pub enum WindowThreadMessage {
        Command(WindowThreadCommand),
}

pub enum WindowThreadCommand {
        Exit,
        SetCursorState(CursorState),
        SetWindowMode(WindowMode),
        SetPlayerCameraEnabled(bool),
}

struct LoggerSystem {
        player_transform: SharedValueMaster<TransformComponent>,
}

impl<'a> specs::System<'a> for LoggerSystem {
        type SystemData = (ReadExpect<'a, PlayerResource>, WriteStorage<'a, TransformComponent>);

        fn run(&mut self, (player, mut transforms): Self::SystemData) {
                if let Some(transform) = self.player_transform.receive() {
                        *transforms.get_mut(player.0).unwrap() = transform;
                } else {
                        self.player_transform.emit(*transforms.get(player.0).unwrap());
                }
        }
}

struct SharedValueMaster<T> {
        tx: single_value_channel::Updater<T>,
        rx: single_value_channel::Receiver<Option<T>>,
}

impl<T> SharedValueMaster<T> {
        pub fn new(value: T) -> (Self, SharedValueSlave<T>) {
                let (rx0, tx0) = single_value_channel::channel_starting_with(value);
                let (rx1, tx1) = single_value_channel::channel();

                (Self { tx: tx0, rx: rx1 }, SharedValueSlave { tx: tx1, rx: rx0 })
        }

        pub fn emit(&mut self, value: T) {
                self.tx.update(value).unwrap();
        }

        pub fn receive(&mut self) -> Option<T> {
                self.rx.latest_mut().take()
        }
}

struct SharedValueSlave<T> {
        rx: single_value_channel::Receiver<T>,
        tx: single_value_channel::Updater<Option<T>>,
}

impl<T: Copy> SharedValueSlave<T> {
        #[allow(dead_code)]
        pub fn emit(&mut self, value: T) {
                *self.get_mut() = value;
                self.tx.update(Some(value)).unwrap();
        }

        #[allow(dead_code)]
        pub fn reemit(&mut self) {
                self.tx.update(Some(*self.rx.latest())).unwrap();
        }

        #[allow(dead_code)]
        pub fn get(&mut self) -> &T {
                self.rx.latest()
        }

        #[allow(dead_code)]
        pub fn get_mut(&mut self) -> &mut T {
                self.rx.latest_mut()
        }
}

struct RenderStateManager {
        render_state_switcher: Arc<Mutex<RenderStateSwitcher>>,
        old_render_state: Option<Box<RenderState>>,
        new_render_state: Option<Box<RenderState>>,
        latest_render_state_switch: Instant,
}

impl RenderStateManager {
        fn new(render_state_switcher: Arc<Mutex<RenderStateSwitcher>>) -> Self {
                Self {
                        render_state_switcher,
                        old_render_state: None,
                        new_render_state: None,
                        latest_render_state_switch: Instant::now(),
                }
        }

        fn get_render_state(&mut self, target_ticktime: f32) -> Option<RenderState> {
                self.try_switch_render_state();

                self.try_interp_render_states(target_ticktime)
        }

        fn try_switch_render_state(&mut self) {
                let mut render_state_switcher = self.render_state_switcher.lock().unwrap();
                if let Some(new_render_state) = render_state_switcher.try_exchange(&mut self.old_render_state) {
                        self.old_render_state = self.new_render_state.replace(new_render_state);
                        self.latest_render_state_switch = Instant::now();
                } else {
                        if let Some(new_render_state) = &mut self.new_render_state {
                                new_render_state.asset_manager.as_mut().unwrap().clear_events();
                        }
                }
        }

        fn try_interp_render_states(&self, target_ticktime: f32) -> Option<RenderState> {
                match (&self.old_render_state, &self.new_render_state) {
                        (Some(old_render_state), Some(new_render_state)) => {
                                let elapsed = self.latest_render_state_switch.elapsed();
                                let t = f32::clamp(elapsed.as_secs_f32() / target_ticktime, 0.0, 1.0);

                                Some(RenderState::interpolate(old_render_state, new_render_state, t))
                        },
                        (None, Some(new_render_state)) => Some(RenderState::clone(new_render_state)),
                        _ => None,
                }
        }
}

struct MatrixStack {
        stack: Vec<Mat4>,
}

impl MatrixStack {
        fn new() -> Self {
                Self { stack: Vec::new() }
        }

        fn push(&mut self, matrix: Mat4) -> Mat4 {
                let transformed = match self.stack.last() {
                        Some(l) => l * matrix,
                        None => matrix,
                };

                self.stack.push(transformed);

                transformed
        }

        fn pop(&mut self) {
                self.stack.pop().expect("Tried to pop matrix of empty MatrixStack!");
        }
}

fn update_transforms(
        query: Query<(Entity, &Transform, &ModelInstance), Changed<Transform>>,
        asset_manager: Res<AssetManager>,
        mut transform_manager: ResMut<TransformManager>,
) {
        transform_manager.on_update();

        let mut matrix_stack = MatrixStack::new();
        for (e, transform, model_instance) in query.iter() {
                matrix_stack.push(transform.to_matrix());
                process_model_instance(
                        &mut matrix_stack,
                        &mut transform_manager,
                        &asset_manager,
                        model_instance,
                );
                matrix_stack.pop();
        }
}

fn process_model_instance(
        matrix_stack: &mut MatrixStack,
        transform_manager: &mut TransformManager,
        asset_manager: &AssetManager,
        model_instance: &ModelInstance,
) {
        let model = &asset_manager.models()[model_instance.model];

        let transform = matrix_stack.push(model.base_transform);
        transform_manager.set_transform(model_instance.transform, &transform);

        for child_model_instance in &model_instance.children {
                process_model_instance(matrix_stack, transform_manager, asset_manager, child_model_instance);
        }

        matrix_stack.pop();
}

fn process_actions(
        mut commands: Commands,
        delta_time: Res<DeltaTime>,
        player: Res<Player>,
        action_receiver: NonSend<ActionReceiver>,
        mut control_flow: ResMut<ControlFlow>,
        mut window_mode: ResMut<WindowMode>,
        mut cursor_state: ResMut<CursorState>,
        // TODO: make commands by observing modifications to Res<WindowMode>, etc.
        mut window_commands: ResMut<Vec<WindowCommand>>,
        mut transforms: Query<&mut Transform>,
) {
        let mut desired_dir = Vec3::new(0.0, 0.0, 0.0);

        for (action_id, strength) in action_receiver.receive() {
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
                let mut player_transform = transforms.get_mut(player.0).unwrap();

                let player_hor_orien = UnitQuat::new_normalize(Quat::new(
                        player_transform.orien.as_vector().w,
                        0.0,
                        player_transform.orien.as_vector().y,
                        0.0,
                ));

                let move_amount = PLAYER_MOVEMENT_SPEED * delta_time.0;
                let move_vector = player_hor_orien * desired_dir.normalize() * move_amount;

                player_transform.pos += move_vector;
        }
}

fn relative_transform_updater(
        mut commands: Commands,
        children: Query<(Entity, &RelativeTransform, &Parent)>,
        transforms: Query<&Transform>,
) {
        for (child, rel_transform, parent) in children.iter() {
                let parent_transform = transforms.get(parent.0).unwrap();
                let mut new_child_transform = parent_transform.clone();
                new_child_transform.pos += rel_transform.0.pos;
                new_child_transform.orien *= rel_transform.0.orien;
                new_child_transform.scale += rel_transform.0.scale;
                commands.entity(child).insert(new_child_transform);
        }
}

fn init_new_transforms(mut commands: Commands, new_transforms: Query<(Entity, &Transform), Added<Transform>>) {
        for (e, new_transform) in new_transforms.iter() {
                commands.entity(e).insert(OldTransform(*new_transform));
                commands.entity(e).insert(InterpTransform(*new_transform));
        }
}

fn persist_transforms(mut transforms: Query<(&Transform, &mut OldTransform)>) {
        for (transform, mut old_transform) in transforms.iter_mut() {
                old_transform.0 = *transform;
        }
}

fn interpolate_transforms(world: &mut World, t: f32) {
        let mut query = world.query::<(&OldTransform, &Transform, &mut InterpTransform)>();

        for (old_transform, new_transform, mut interp_transform) in query.iter_mut(world) {
                interp_transform.0 = Transform::interp(&old_transform.0, new_transform, t);
        }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct InterpTransform(pub Transform);

enum WindowCommand {
        SetCursorState(CursorState),
        SetWindowMode(WindowMode),
}
