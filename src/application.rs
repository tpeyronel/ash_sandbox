use std::{
        path::Path,
        rc::Rc,
        time::{Duration, Instant},
};

use crossbeam_channel::Receiver;
use shader_resource_derive::ShaderStruct;
use tps_counter::TPSCounter;

use crate::{
        actions::*,
        application_config::ApplicationConfig,
        asset_manager::*,
        components::{
                ActiveCamera, ActiveCameraControlEnabled, AngularVelocity, Billboard, Children, ClearWorldTrackers,
                DirectionalLight, EnableAngularVelocity, EnableOrbitalVelocity, Force, GlobalTransform,
                ImguiWantCaptureKeyboard, ImguiWantCaptureMouse, InterpScalar, Mass, OrbitalVelocity, Parent, Player,
                PointLight, PreviousGlobalTransform, ProjectionCamera, ShouldQuit, Spotlight, TickTime, Transform,
                UpdateBegin, UpdateTime, UpdateTimeAccumulator, Velocity,
        },
        constants::{FONT_SIZE, PLAYER_MOVEMENT_SPEED, ROTATION_PER_SECOND},
        euler_angles::EulerAngles,
        hashmap::{GetOrInsertDefault, HashMap},
        imgui_util,
        input_manager::{
                ActionEvent, ActionStrength, InputBindingMap, InputManager, KeyBindingType, KeyCode, KeyState,
                MouseMotionType,
        },
        model_instance_manager::CmdAddModelInstanceByName,
        my_glm::*,
        renderer::Renderer,
        skybox::Skybox,
        vk::vk_renderer::VkRenderer,
        window_manager::WindowManager,
        AnyResult,
};
use bevy_ecs::{
        event::Events,
        prelude::*,
        schedule::{RunOnce, ShouldRun},
        system::{Resource, SystemParam},
};
#[allow(unused_imports)]
use log::{error, info, trace};
use serde::{Deserialize, Serialize};
use winit::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Fullscreen, Window, WindowBuilder},
};

#[allow(dead_code)]
pub struct Application {
        event_loop: Option<EventLoop<()>>,

        target_tick_time: f32,
        world: World,
        schedule: Schedule,
        window: Rc<Window>,
        input_manager: InputManager,

        renderer: Box<dyn Renderer>,

        player_camera_enabled: bool,

        tps_counter: TPSCounter,
        frame_begin: Instant,
        delta_time: f32,
}

impl Application {
        pub fn new() -> AnyResult<Self> {
                let config = ApplicationConfig::from_file(Path::new("config.json"))?;

                let event_loop = EventLoop::new()?;
                let fullscreen_video_mode = event_loop.primary_monitor().unwrap().video_modes().next().unwrap();
                let window = Rc::new(WindowBuilder::new()
                        .with_fullscreen(match config.window_mode {
                                WindowMode::Windowed => None,
                                WindowMode::Borderless => Some(Fullscreen::Borderless(None)),
                                WindowMode::Fullscreen => Some(Fullscreen::Exclusive(fullscreen_video_mode.clone())),
                        })
                        .with_visible(false)
                        .with_min_inner_size(winit::dpi::PhysicalSize::<u32> {
                                width: 144,
                                height: 144,
                        })
                        .build(&event_loop)?);
                trace!("Created window");

                let mut imgui_manager = ImguiManager::new(&window);

                imgui_manager.add_callback(move |ui, world| {
                        ui.window("Hello world")
                                .size([300.0, 100.0], imgui::Condition::FirstUseEver)
                                .build(|| {
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
                                                tps_counter.tick_time().as_secs_f32() * 1000.0,
                                        ));
                                        ui.text(format!("Mouse pos: ({:.1},{:.1})", mouse_pos[0], mouse_pos[1]));
                                        ui.separator();

                                        let mut player = world.entity_mut(world.get_resource::<Player>().unwrap().0);
                                        imgui_util::transform_mut(ui, &mut player.get_mut::<Transform>().unwrap());

                                        let mut camera =
                                                world.entity_mut(world.get_resource::<ActiveCamera>().unwrap().0);
                                        imgui_util::euler_angles_mut(ui, &mut camera.get_mut::<EulerAngles>().unwrap());

                                        ui.separator();
                                        ui.text("Shader Settings");
                                        let mut shader_settings = world.get_resource_mut::<ShaderSettings>().unwrap();
                                        imgui_util::shader_settings(ui, &mut shader_settings);
                                        ui.separator();

                                        let mut enable_orbital_velocity =
                                                world.get_resource_mut::<EnableOrbitalVelocity>().unwrap();
                                        ui.checkbox("enable orbital velocity", &mut enable_orbital_velocity.0);

                                        let mut enable_angular_velocity =
                                                world.get_resource_mut::<EnableAngularVelocity>().unwrap();
                                        ui.checkbox("enable angular velocity", &mut enable_angular_velocity.0);

                                        if let Some(_) = ui.tree_node("directional light") {
                                                let mut dir_light = world
                                                        .query::<&mut DirectionalLight>()
                                                        .iter_mut(world)
                                                        .next()
                                                        .unwrap();

                                                imgui_util::dir_light_mut(ui, &mut dir_light);
                                        };

                                        if let Some(_) = ui.tree_node("point light") {
                                                let (mut point_light_transform, mut point_light) = world
                                                        .query::<(&mut Transform, &mut PointLight)>()
                                                        .iter_mut(world)
                                                        .next()
                                                        .unwrap();

                                                imgui_util::transform_mut(ui, &mut point_light_transform);
                                                imgui_util::point_light_mut(ui, &mut point_light);
                                        };

                                        if let Some(_) = ui.tree_node("spotlight") {
                                                let mut spotlight =
                                                        world.query::<&mut Spotlight>().iter_mut(world).next().unwrap();

                                                imgui_util::spotlight_mut(ui, &mut spotlight);
                                        };

                                        let mut asset_manager = world.get_resource_mut::<AssetManager>().unwrap();

                                        if let Some(_) = ui.tree_node("materials") {
                                                let mut changed_materials = Vec::new();

                                                for (material_id, material) in &asset_manager.assets.materials {
                                                        let material_name = format!(
                                                                "{:?} - {}",
                                                                material_id,
                                                                material.name.as_deref().unwrap_or("unknown")
                                                        );

                                                        if let Some(_) = ui.tree_node(material_name) {
                                                                let mut material = material.clone();
                                                                imgui_util::material_mut(ui, &mut material);
                                                                changed_materials.push((material_id, material));
                                                        };
                                                }

                                                for (material_id, material) in changed_materials {
                                                        asset_manager.assets.materials[material_id] = material;
                                                }
                                        };
                                });

                        ui.show_demo_window(&mut false);

                        let cursor_state = *world.get_resource::<CursorState>().unwrap();
                        let want_capture_mouse = ui.io().want_capture_mouse && cursor_state == CursorState::Normal;
                        world.get_resource_mut::<ImguiWantCaptureMouse>().unwrap().0 = want_capture_mouse;
                        world.get_resource_mut::<ImguiWantCaptureKeyboard>().unwrap().0 = ui.io().want_capture_keyboard;
                });
                trace!("Initialized ImGui");

                let mut world = World::default();
                world.insert_non_send(imgui_manager);
                world.insert_resource(TickTime(1.0 / config.tps as f32));
                world.insert_resource(ImguiWantCaptureMouse(false));
                world.insert_resource(ImguiWantCaptureKeyboard(false));
                world.insert_resource(TPSCounter::new(20));

                let (mut asset_manager, asset_manager_event_rx) = Self::init_asset_manager()?;

                let skybox = asset_manager.insert_cubemap(Cubemap::Faces(Image::from_files_cubemap(
                        Some("skybox".into()),
                        [
                                Path::new("res/image/skybox/right.png"),
                                Path::new("res/image/skybox/left.png"),
                                Path::new("res/image/skybox/up.png"),
                                Path::new("res/image/skybox/down.png"),
                                Path::new("res/image/skybox/front.png"),
                                Path::new("res/image/skybox/back.png"),
                        ],
                        ColorSpace::Srgb,
                )?));
                // world.insert_resource(Skybox(skybox));

                // asset_manager.add_image(Image::from_file(
                //         Path::new("res/image/results/wide_street.DDS"),
                //         ColorSpace::Linear,
                // )?);

                let wide_street_cubemap = asset_manager.insert_cubemap(Cubemap::Equirectangular(Image::from_file(
                        // Path::new("res/image/wide_street.exr"),
                        Path::new("res/image/results/wide_street.DDS"),
                        ColorSpace::Linear,
                )?));
                world.insert_resource(Skybox(wide_street_cubemap));

                world.insert_resource(asset_manager);
                world.insert_resource(ShouldQuit(false));
                world.insert_resource(EnableOrbitalVelocity(true));
                world.insert_resource(EnableAngularVelocity(true));
                world.insert_resource(ShaderSettings::default());

                let cursor_state = CursorState::Normal;
                let window_mode = WindowMode::Windowed;
                world.insert_resource(cursor_state);
                world.insert_resource(window_mode);
                world.insert_non_send(WindowManager::new(
                        Rc::clone(&window),
                        fullscreen_video_mode,
                        window_mode,
                        cursor_state,
                ));

                world.insert_resource(UpdateTime(0.0));
                world.insert_resource(UpdateTimeAccumulator(world.get_resource::<TickTime>().unwrap().0));

                let mut schedule = Self::create_schedule();
                register_event::<ActionEvent>(&mut world, &mut schedule);

                let renderer = Box::new(VkRenderer::new(
                        Rc::clone(&window),
                        &mut world.get_non_send_resource_mut::<ImguiManager>().unwrap().imgui_context,
                        asset_manager_event_rx,
                )?);

                let mut input_manager = InputManager::new();

                let mut input_map = InputBindingMap::new();

                input_map.bind_key(EXIT, KeyCode::Escape, KeyBindingType::Simple(KeyState::Released));
                input_map.bind_key(TOGGLE_CURSOR, KeyCode::KeyT, KeyBindingType::Simple(KeyState::Released));
                input_map.bind_key(
                        CYCLE_WINDOW_MODE,
                        KeyCode::F11,
                        KeyBindingType::Simple(KeyState::Released),
                );

                input_map.bind_key(MOVE_FORWARD, KeyCode::KeyW, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_BACKWARD, KeyCode::KeyS, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_RIGHTWARD, KeyCode::KeyD, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_LEFTWARD, KeyCode::KeyA, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_UPWARD, KeyCode::Space, KeyBindingType::Continuous);
                input_map.bind_key(MOVE_DOWNARD, KeyCode::ShiftLeft, KeyBindingType::Continuous);

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

                Ok(Self {
                        event_loop: Some(event_loop),
                        target_tick_time: 1.0 / config.tps as f32,
                        world,
                        schedule,
                        window,
                        input_manager,

                        renderer,

                        player_camera_enabled: false,

                        tps_counter: TPSCounter::new(20),
                        frame_begin: Instant::now(),
                        delta_time: 0.0,
                })
        }

        pub fn run(mut self) -> AnyResult<()> {
                self.window.set_visible(true);
                self.world.insert_resource(UpdateBegin(Instant::now()));

                let event_loop = self.event_loop.take().unwrap();
                event_loop.set_control_flow(ControlFlow::Poll);
                event_loop.run(move |event, target| {
                        let mut quit = false;

                        self.on_winit_event(event, &mut quit)
                                .expect("Error ocurred in render loop");

                        if quit {
                                target.exit();
                        }
                })?;

                Ok(())
        }

        fn init_asset_manager() -> AnyResult<(AssetManager, Receiver<AssetManagerEvent>)> {
                let (mut asset_manager, event_rx) = AssetManager::new()?;

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

                let _equi_to_cube_shader = asset_manager
                        .load_shader_from_yaml(Path::new("res/shader/equi_to_cube_shader/equi_to_cube_shader.yaml"))?;

                let _shadow_map_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/shadow_map/shadow_map.yaml"))?;

                let _hdr_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/hdr_shader/hdr_shader.yaml"))?;

                let _cube_shadow_map_shader = asset_manager
                        .load_shader_from_yaml(Path::new("res/shader/cube_shadow_map/cube_shadow_map.yaml"))?;

                let _basic_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/basic_shader/basic_shader.yaml"))?;
                let pbr_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/pbr_shader/pbr_shader.yaml"))?;
                let color_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/color_shader/color_shader.yaml"))?;
                let billboard_shader = asset_manager
                        .load_shader_from_yaml(Path::new("res/shader/billboard_shader/billboard_shader.yaml"))?;

                let _model_colt = asset_manager.import_gltf_file(Path::new("res/model/new-colt/colt.gltf"))?;
                let _model_grass_plane =
                        asset_manager.import_gltf_file(Path::new("res/model/grass-plane/grass-plane.gltf"))?;
                let _model_sphere = asset_manager.import_gltf_file(Path::new("res/model/sphere/sphere.gltf"))?;
                let _iron_sphere = asset_manager.import_gltf_file_with_shader(
                        Path::new("res/model/iron-sphere/iron-sphere.gltf"),
                        pbr_shader,
                )?;
                let _model_icosphere =
                        asset_manager.import_gltf_file(Path::new("res/model/icosphere/icosphere.gltf"))?;
                let _model_lit_icosphere = asset_manager.import_gltf_file_with_shader(
                        Path::new("res/model/lit-icosphere/lit-icosphere.gltf"),
                        color_shader,
                )?;
                let _model_backpack = asset_manager.import_gltf_file(Path::new("res/model/backpack/backpack.gltf"))?;
                let _model_landscape = asset_manager.import_gltf_file_with_shader(
                        Path::new("res/model/landscape/landscape.gltf"),
                        billboard_shader,
                )?;
                let _model_health_bar = asset_manager.import_gltf_file_with_shader(
                        Path::new("res/model/health-bar/health-bar.gltf"),
                        billboard_shader,
                )?;
                let _model_brick_wall =
                        asset_manager.import_gltf_file(Path::new("res/model/brick-wall/brick-wall.gltf"))?;

                trace!("Initialized AssetManager");
                Ok((asset_manager, event_rx))
        }

        fn create_schedule() -> Schedule {
                let startup_schedule = Schedule::default().with_run_criteria(RunOnce::default()).with_stage(
                        StartupStage::Startup,
                        SystemStage::parallel().with_system(spawn_entities),
                );

                let first_schedule = Schedule::default().with_stage(
                        FirstStage::First,
                        SystemStage::parallel()
                                .with_system(camera_control_system.label("camera-control-system"))
                                .with_system(
                                        unfixed_action_handling_system
                                                .label("unfixed-action-handling-system")
                                                .after("camera-control-system"),
                                )
                                .with_system(delta_time_system.label("delta-time-system")),
                );

                let update_schedule = Schedule::default()
                        .with_run_criteria(should_update.system())
                        .with_stage(
                                UpdateStage::PreUpdate,
                                SystemStage::parallel()
                                        .with_system(update_tps_counter)
                                        .with_system(hierarchy_maintenance_system.label("hierarchy-maintenance"))
                                        .with_system(renormalize_quaternions.after("hierarchy-maintenance"))
                                        .with_system(init_new_transforms.label("init-new-transforms")),
                        )
                        .with_stage_after(
                                UpdateStage::PreUpdate,
                                UpdateStage::Update,
                                SystemStage::parallel()
                                        .with_system(persist_transforms.label("persist-transforms"))
                                        .with_system(
                                                process_actions.label("process-actions").after("persist-transforms"),
                                        )
                                        .with_system(
                                                apply_euler_angles.label("apply-euler-angles").after("process-actions"),
                                        )
                                        .with_system(integrate_force.label("linear-force").after("apply-euler-angles"))
                                        .with_system(
                                                integrate_linear_velocity
                                                        .label("linear-velocity")
                                                        .after("linear-force"),
                                        )
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
                                                billboard_system.label("billboard-sytem").after("orbital-velocity"),
                                        )
                                        .with_system(
                                                global_transform_system
                                                        .label("global-transform-system")
                                                        .after("billboard-sytem"),
                                        ),
                        )
                        .with_stage_after(UpdateStage::Update, UpdateStage::PostUpdate, SystemStage::parallel());

                let render_schedule = Schedule::default().with_stage(
                        RenderStage::Render,
                        SystemStage::parallel()
                                .with_system(window_system.label("window-system"))
                                .with_system(apply_euler_angles.label("apply-euler-angles"))
                                .with_system(
                                        interpolate_transforms
                                                .label("interpolate-transforms")
                                                .after("apply-euler-angles"),
                                ),
                );

                let cleanup_stage = SystemStage::parallel().with_run_criteria(should_perform_cleanup_system);

                Schedule::default()
                        .with_stage(CoreStage::Startup, startup_schedule)
                        .with_stage_after(CoreStage::Startup, CoreStage::First, first_schedule)
                        .with_stage_after(CoreStage::Startup, CoreStage::Update, update_schedule)
                        .with_stage_after(CoreStage::Update, CoreStage::Render, render_schedule)
                        .with_stage_after(CoreStage::Render, CoreStage::Cleanup, cleanup_stage)
        }

        fn on_winit_event(&mut self, event: winit::event::Event<()>, quit: &mut bool) -> AnyResult<()> {
                self.world
                        .get_non_send_resource_mut::<ImguiManager>()
                        .unwrap()
                        .handle_winit_event(&self.window, &event);

                match event {
                        Event::DeviceEvent { event, .. } => {
                                self.input_manager.on_device_event(&event);
                        },
                        Event::WindowEvent { window_id, event } if self.window.id() == window_id => {
                                self.on_window_event(event, quit);
                        },
                        Event::AboutToWait => self.update(quit)?, // TODO: is this event OK?
                        Event::LoopExiting => self.on_quit(),
                        _ => (),
                }

                Ok(())
        }

        fn on_window_event(&mut self, window_event: WindowEvent, quit: &mut bool) {
                match window_event {
                        /* WindowEvent::ModifiersChanged(modifiers_state) => {
                                self.input_manager.on_modifiers_changed(modifiers_state)
                        } */
                        WindowEvent::Focused(focused) => {
                                self.world
                                        .get_non_send_resource_mut::<WindowManager>()
                                        .unwrap()
                                        .on_window_focused(focused);
                        },
                        WindowEvent::Resized(new_size) => {
                                self.renderer.on_window_resize(new_size.width, new_size.height);
                        },
                        WindowEvent::CloseRequested => {
                                *quit = true;
                        },
                        _ => {},
                };
        }

        fn update(&mut self, quit: &mut bool) -> AnyResult<()> {
                self.dispatch_actions_to_world();

                self.schedule.run(&mut self.world);

                let mut imgui_manager = self.world.remove_non_send::<ImguiManager>().unwrap();
                imgui_manager.build_imgui_ui(&self.window, &mut self.world)?;
                self.renderer.draw_world(&mut self.world, imgui_manager.render())?;
                self.world.insert_non_send(imgui_manager);

                *quit = self.world.get_resource::<ShouldQuit>().unwrap().0;

                if let Some(_) = self.world.remove_resource::<ClearWorldTrackers>() {
                        self.world.clear_trackers();
                }

                Ok(())
        }

        fn dispatch_actions_to_world(&mut self) {
                let imgui_want_capture_mouse = self.world.get_resource::<ImguiWantCaptureMouse>().unwrap().0;
                self.input_manager.set_ignore_mouse(imgui_want_capture_mouse);

                let imgui_want_capture_keyboard = self.world.get_resource::<ImguiWantCaptureKeyboard>().unwrap().0;
                self.input_manager.set_ignore_keyboard(imgui_want_capture_keyboard);

                let should_dispatch_actions = self.should_dispatch_actions();

                let new_action_events = self.input_manager.drain_events();
                if should_dispatch_actions {
                        let mut action_events = self.world.get_resource_mut::<Events<ActionEvent>>().unwrap();
                        for new_action_event in new_action_events {
                                action_events.send(new_action_event);
                        }
                }

                let pollable_actions = if should_dispatch_actions {
                        self.input_manager.poll_actions()
                } else {
                        HashMap::new()
                };
                self.world.insert_resource(PollableActions(pollable_actions));
        }

        fn should_dispatch_actions(&self) -> bool {
                let window_manager = self.world.get_non_send_resource::<WindowManager>().unwrap();
                if !window_manager.is_focused() {
                        return false;
                }

                return true;
        }

        fn on_quit(&mut self) {
                self.renderer.destroy().unwrap();
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
                .insert(Transform::from_translation(Vec3::new(0.0, -2.0, 1.5)))
                .id();
        commands.insert_resource(Player(player));

        let player_head = commands
                .spawn()
                .insert(Parent(player))
                .insert(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)))
                .insert(EulerAngles::new(0.0, 0.0, 0.0))
                .insert(ProjectionCamera::new(90.0f32.to_radians(), 1.0, 0.1, 100.0))
                .insert(Spotlight {
                        radius_angle: 45.0f32.to_radians(),
                        inner_radius_percentage: 0.5,
                        color: Vec3::ZERO,
                        kc: 1.0,
                        kl: 0.0,
                        kq: 1.0,
                })
                .id();
        commands.insert_resource(ActiveCamera(player_head));
        commands.insert_resource(ActiveCameraControlEnabled(true));

        // let colt = commands
        //         .spawn()
        //         .insert(Transform::from_translation(Vec3::new(2.5, 1.0, 0.0)))
        //         .insert(AngularVelocity(Vec3::Y * 45.0f32.to_radians()))
        //         .insert(OrbitalVelocity {
        //                 origin: Vec3::new(0.0, 2.5, 0.0),
        //                 velocity: (Vec3::X + Vec3::Y).normalize() * -22.5f32.to_radians(),
        //         })
        //         .id();

        // commands.add(CmdAddModelInstanceByName::from_str(colt, "colt"));

        let icosphere = commands
                .spawn()
                .insert(Transform {
                        translation: Vec3::new(0.0, 0.0, 0.0),
                        scale: Vec3::splat(1.0),
                        ..Transform::identity()
                })
                .insert(Force(Vec3::new(0.0, 0.0, 0.0)))
                .insert(Mass(1.0))
                .insert(Velocity(Vec3::new(0.0, 0.0, 0.0)))
                .id();

        commands.add(CmdAddModelInstanceByName::from_str(icosphere, "iron-sphere"));

        // let backpack = commands
        //         .spawn()
        //         .insert(Transform {
        //                 translation: Vec3::new(2.0, 1.0, 0.0),
        //                 scale: Vec3::new(5.0, 5.0, 5.0),
        //                 ..Transform::identity()
        //         })
        //         .insert(AngularVelocity(Vec3::Y * 22.5f32.to_radians()))
        //         .insert(OrbitalVelocity {
        //                 origin: Vec3::new(0.0, 0.0, 0.0),
        //                 velocity: Vec3::Y * -22.5f32.to_radians(),
        //         })
        //         .id();

        // commands.add(CmdAddModelInstanceByName::from_str(backpack, "backpack"));

        // let brick_wall = commands
        //         .spawn()
        //         .insert(Transform {
        //                 translation: Vec3::new(4.0, 0.0, -4.0),
        //                 scale: Vec3::splat(0.25),
        //                 rotation: Quat::from_axis_angle(Vec3::Y, -45.0f32.to_radians()),
        //         })
        //         .id();

        // commands.add(CmdAddModelInstanceByName::from_str(brick_wall, "brick-wall"));

        // TODO: make only some models (or materials?) cast shadow.

        // let billboard = commands
        //         .spawn()
        //         .insert(Transform {
        //                 translation: Vec3::new(0.0, 0.25, 0.0),
        //                 scale: Vec3::splat(0.5) * Vec3::new(1.0, 0.25, 1.0),
        //                 ..Transform::identity()
        //         })
        //         .insert(Billboard)
        //         .insert(Parent(backpack))
        //         .id();

        // commands.add(CmdAddModelInstanceByName::from_str(billboard, "health-bar"));

        // let static_billboard = commands
        //         .spawn()
        //         .insert(Transform {
        //                 translation: Vec3::new(0.0, 1.5, 0.0),
        //                 scale: Vec3::splat(2.5) * Vec3::new(1.0, 0.5, 1.0),
        //                 ..Transform::identity()
        //         })
        //         .insert(Billboard)
        //         .id();

        // commands.add(CmdAddModelInstanceByName::from_str(static_billboard, "health-bar"));

        // let grass_plane = commands
        //         .spawn()
        //         .insert(Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)))
        //         .id();

        // commands.add(CmdAddModelInstanceByName::from_str(grass_plane, "grass-plane"));

        let light = commands
                .spawn()
                .insert(Transform {
                        translation: Vec3::new(1.5, 1.0, 1.5),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::splat(0.25),
                })
                .insert(PointLight {
                        color: Vec3::new(0.9, 1.0, 0.9),
                        kc: 1.0,
                        kl: 0.0,
                        kq: 1.0,
                })
                // .insert(OrbitalVelocity {
                //         origin: Vec3::splat(0.0),
                //         velocity: Vec3::Y * 45.0f32.to_radians(),
                // })
                .id();

        // commands.add(CmdAddModelInstanceByName::from_str(light, "lit-icosphere"));

        let _dir_light = commands
                .spawn()
                .insert(DirectionalLight {
                        direction: Vec3::new(1.0, -1.0, 0.0),
                        color: Vec3::ZERO,
                        intensity: 1.0,
                })
                .id();
}

fn delta_time_system(
        mut update_begin: ResMut<UpdateBegin>,
        mut update_time: ResMut<UpdateTime>,
        mut imgui_manager: NonSendMut<ImguiManager>,
) {
        let prev_update_begin = std::mem::replace(&mut update_begin.0, Instant::now());
        let dt = update_begin.0 - prev_update_begin;
        update_time.0 = dt.as_secs_f32();
        imgui_manager.on_delta_time_updated(dt);
}

fn should_update(
        mut commands: Commands,
        mut accumulator: ResMut<UpdateTimeAccumulator>,
        update_time: Res<UpdateTime>,
        tick_time: Res<TickTime>,
) -> ShouldRun {
        accumulator.0 += update_time.0;

        if accumulator.0 >= tick_time.0 {
                accumulator.0 -= tick_time.0;
                commands.insert_resource(ClearWorldTrackers);
                ShouldRun::Yes
        } else {
                ShouldRun::No
        }
}

fn process_actions(
        mut commands: Commands,
        tick_time: Res<TickTime>,
        player: Res<Player>,
        camera: Res<ActiveCamera>,
        mut actions: ActionReader,
        mut should_quit: ResMut<ShouldQuit>,
        mut cursor_state: ResMut<CursorState>,
        mut window_mode: ResMut<WindowMode>,
        global_transform_query: Query<&GlobalTransform>,
        mut transform_query: Query<&mut Transform>,
) {
        let mut desired_dir = Vec3::new(0.0, 0.0, 0.0);

        for (action_id, strength) in actions.read(tick_time.0) {
                match *action_id {
                        MOVE_FORWARD => desired_dir.z -= strength.0,
                        MOVE_BACKWARD => desired_dir.z += strength.0,
                        MOVE_RIGHTWARD => desired_dir.x += strength.0,
                        MOVE_LEFTWARD => desired_dir.x -= strength.0,
                        MOVE_UPWARD => desired_dir.y += strength.0,
                        MOVE_DOWNARD => desired_dir.y -= strength.0,
                        EXIT => {
                                should_quit.0 = true;
                        },
                        TOGGLE_CURSOR => {
                                *cursor_state = match *cursor_state {
                                        CursorState::Normal => CursorState::Hidden,
                                        CursorState::Hidden => CursorState::Normal,
                                };
                        },
                        CYCLE_WINDOW_MODE => {
                                *window_mode = match *window_mode {
                                        WindowMode::Windowed => WindowMode::Borderless,
                                        WindowMode::Borderless => WindowMode::Fullscreen,
                                        WindowMode::Fullscreen => WindowMode::Windowed,
                                };
                        },
                        _ => (),
                }
        }

        if desired_dir.length_squared() > f32::EPSILON {
                let camera_orien = global_transform_query.get(camera.0).unwrap().0.rotation;
                let camera_hor_orien = Quat::from_xyzw(0.0, camera_orien.y, 0.0, camera_orien.w).normalize();

                let move_vector = camera_hor_orien * desired_dir * PLAYER_MOVEMENT_SPEED;

                let mut player_transform = transform_query.get_mut(player.0).unwrap();
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

fn camera_control_system(
        cursor_state: Res<CursorState>,
        mut camera_control_enabled: ResMut<ActiveCameraControlEnabled>,
) {
        let enable_camera_control = *cursor_state == CursorState::Hidden;

        if camera_control_enabled.0 != enable_camera_control {
                camera_control_enabled.0 = enable_camera_control;
        }
}

fn unfixed_action_handling_system(
        mut commands: Commands,
        mut euler_angles_query: Query<&mut EulerAngles>,
        mut actions: ActionReader,
        active_camera: Res<ActiveCamera>,
        camera_control_enabled: Res<ActiveCameraControlEnabled>,
        update_time: Res<UpdateTime>,
) {
        let mut camera_orien = euler_angles_query.get_mut(active_camera.0).unwrap();

        for (action_id, strength) in actions.read(update_time.0) {
                // if self.window_state.cursor_state != CursorState::Hidden {
                if !camera_control_enabled.0 {
                        continue;
                }

                match *action_id {
                        YAW_POSITIVE => camera_orien.yaw_by(strength.0 * ROTATION_PER_SECOND),
                        YAW_NEGATIVE => camera_orien.yaw_by(-strength.0 * ROTATION_PER_SECOND),
                        PITCH_POSITIVE => camera_orien.pitch_by(strength.0 * ROTATION_PER_SECOND),
                        PITCH_NEGATIVE => camera_orien.pitch_by(-strength.0 * ROTATION_PER_SECOND),
                        ROLL_POSITIVE => camera_orien.roll_by(strength.0 * ROTATION_PER_SECOND),
                        ROLL_NEGATIVE => camera_orien.roll_by(-strength.0 * ROTATION_PER_SECOND),
                        _ => continue,
                }
        }
}

struct PollableActions(pub HashMap<ActionId, ActionStrength>);

#[derive(SystemParam)]
struct ActionReader<'w, 's> {
        action_events: EventReader<'w, 's, ActionEvent>,
        pollable_actions: Res<'w, PollableActions>,
}

impl<'w, 's> ActionReader<'w, 's> {
        pub fn read<'a>(&mut self, coefficient: f32) -> impl Iterator<Item = (&ActionId, ActionStrength)> + '_ {
                let action_events = self.action_events.iter().map(|e| (&e.action_id, e.strength));
                let pollable_actions = self
                        .pollable_actions
                        .0
                        .iter()
                        .map(move |(id, s)| (id, ActionStrength(s.0 * coefficient)));

                pollable_actions.chain(action_events)
        }
}

fn update_tps_counter(mut tps_counter: ResMut<TPSCounter>) {
        tps_counter.tick();
}

fn renormalize_quaternions(mut transforms: Query<&mut Transform, Changed<Transform>>) {
        for mut transform in transforms.iter_mut() {
                transform.rotation = transform.rotation.normalize();
        }
}

fn persist_transforms(mut transforms: Query<(&GlobalTransform, &mut PreviousGlobalTransform)>) {
        for (transform, mut old_transform) in transforms.iter_mut() {
                old_transform.0 = transform.0;
        }
}

fn window_system(
        mut window_manager: NonSendMut<WindowManager>,
        mut imgui_manager: NonSendMut<ImguiManager>,
        window_mode: Res<WindowMode>,
        cursor_state: Res<CursorState>,
) {
        if window_mode.is_changed() {
                window_manager.set_window_mode(*window_mode);
        }

        if cursor_state.is_changed() {
                window_manager.set_cursor_state(*cursor_state);

                imgui_manager
                        .imgui_context
                        .io_mut()
                        .config_flags
                        .set(imgui::ConfigFlags::NO_MOUSE, *cursor_state == CursorState::Hidden);
        }
}

fn apply_euler_angles(mut query: Query<(&EulerAngles, &mut Transform)>) {
        for (euler_angles, mut transform) in query.iter_mut() {
                transform.rotation = euler_angles.to_quat();
        }
}

fn integrate_force(mut query: Query<(&Force, &Mass, &mut Velocity)>, tick_time: Res<TickTime>) {
        for (force, mass, mut velocity) in query.iter_mut() {
                let momentum = force.0 * tick_time.0;
                let delta_velocity = momentum / mass.0;
                velocity.0 += delta_velocity;
        }
        // for (force, mass, mut velocity) in query.iter_mut() {
        //         let acceleration = force.0 / mass.0;
        //         velocity.0 += acceleration * tick_time.0;
        // }
}

fn integrate_linear_velocity(mut query: Query<(&Velocity, &mut Transform)>, tick_time: Res<TickTime>) {
        for (velocity, mut transform) in query.iter_mut() {
                transform.translation += velocity.0 * tick_time.0;
        }
}

fn integrate_angular_velocities(
        mut query: Query<(&AngularVelocity, &mut Transform)>,
        enable_angular_velocity: Res<EnableAngularVelocity>,
        tick_time: Res<TickTime>,
) {
        if !enable_angular_velocity.0 {
                return;
        }

        for (angular_velocity, mut transform) in query.iter_mut() {
                transform.rotation *= Quat::from_scaled_axis(angular_velocity.0 * tick_time.0);
        }
}

fn integrate_orbital_velocities(
        mut query: Query<(&OrbitalVelocity, &mut Transform)>,
        enable_orbital_velocity: Res<EnableOrbitalVelocity>,
        tick_time: Res<TickTime>,
) {
        if !enable_orbital_velocity.0 {
                return;
        }

        for (orbital_velocity, mut transform) in query.iter_mut() {
                let orbital_pos = transform.translation - orbital_velocity.origin;
                let orbital_rot = Quat::from_scaled_axis(orbital_velocity.velocity * tick_time.0);
                let new_orbital_pos = orbital_rot * orbital_pos;
                let delta_pos = new_orbital_pos - orbital_pos;

                transform.translation += delta_pos;
        }
}

fn interpolate_transforms(
        mut query: Query<(&PreviousGlobalTransform, &GlobalTransform, &mut InterpGlobalTransform)>,
        accumulator: Res<UpdateTimeAccumulator>,
        tick_time: Res<TickTime>,
        euler_angles_query: Query<&EulerAngles>,
        parent_query: Query<&Parent>,
        camera: Res<ActiveCamera>,
) {
        let t = InterpScalar(accumulator.0 / tick_time.0);

        for (prev_transform, curr_transform, mut interp_transform) in query.iter_mut() {
                interp_transform.0 = Transform::interp(&prev_transform.0, &curr_transform.0, t.0);
        }

        let mut camera_global_rotation = euler_angles_query.get(camera.0).unwrap().to_quat();

        if let Ok(camera_parent) = parent_query.get(camera.0) {
                if let Ok((_, _, parent_transform)) = query.get(camera_parent.0) {
                        camera_global_rotation = parent_transform.0.rotation * camera_global_rotation
                }
        };

        let (_, _, mut interp_transform) = query.get_mut(camera.0).unwrap();
        interp_transform.0.rotation = camera_global_rotation;
}

fn billboard_system(
        mut transform_billboard_query: Query<(Entity, &GlobalTransform, &mut Transform), With<Billboard>>,
        global_transform_query: Query<&GlobalTransform, Without<Billboard>>,
        parent_query: Query<&Parent>,
        camera: Res<ActiveCamera>,
) {
        let camera_transform = global_transform_query.get(camera.0).unwrap();

        for (billboard, global_transform, mut transform) in transform_billboard_query.iter_mut() {
                let mut desired_global_rotation =
                        camera_transform.0.rotation * Quat::from_axis_angle(Vec3::UP, 180.0f32.to_radians());
                // Quat::face_towards(global_transform.0.translation, camera_transform.0.translation, Vec3::UP);

                if let Ok(parent) = parent_query.get(billboard) {
                        if let Ok(parent_global_transform) = global_transform_query.get(parent.0) {
                                desired_global_rotation =
                                        parent_global_transform.0.rotation.inverse() * desired_global_rotation;
                        }
                }

                transform.rotation = desired_global_rotation;
        }
}

fn should_perform_cleanup_system(clear_world_trackers: Option<Res<ClearWorldTrackers>>) -> ShouldRun {
        if clear_world_trackers.is_some() {
                ShouldRun::Yes
        } else {
                ShouldRun::No
        }
}

pub struct ImguiManager {
        imgui_context: imgui::Context,
        imgui_platform: imgui_winit_support::WinitPlatform,
        callbacks: Vec<Box<dyn FnMut(&mut imgui::Ui, &mut World)>>,
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

                imgui_platform.attach_window(imgui_context.io_mut(), window, imgui_winit_support::HiDpiMode::Rounded);

                Self {
                        imgui_context,
                        imgui_platform,
                        callbacks: Vec::new(),
                }
        }

        fn add_callback<T: FnMut(&mut imgui::Ui, &mut World) + 'static>(&mut self, f: T) {
                self.callbacks.push(Box::new(f));
        }

        fn handle_winit_event(&mut self, window: &Window, event: &winit::event::Event<()>) {
                self.imgui_platform
                        .handle_event(self.imgui_context.io_mut(), window, event);
        }

        fn on_delta_time_updated(&mut self, delta_time: Duration) {
                self.imgui_context.io_mut().update_delta_time(delta_time);
        }

        fn build_imgui_ui(
                &mut self,
                window: &winit::window::Window,
                world: &mut World,
        ) -> Result<(), winit::error::ExternalError> {
                self.imgui_platform.prepare_frame(self.imgui_context.io_mut(), window)?;

                let ui = self.imgui_context.new_frame();

                self.callbacks.iter_mut().for_each(|c| c(ui, world));

                Ok(())
        }

        fn render(&mut self) -> &imgui::DrawData {
                self.imgui_context.render()
        }
}

#[derive(Component, Debug, Clone, Copy)]
pub struct InterpGlobalTransform(pub Transform);

#[derive(Clone, Hash, Debug, Eq, PartialEq, StageLabel)]
enum CoreStage {
        Startup,
        First,
        Update,
        Render,
        Cleanup,
}

#[derive(Clone, Hash, Debug, Eq, PartialEq, StageLabel)]
enum StartupStage {
        Startup,
}

#[derive(Clone, Hash, Debug, Eq, PartialEq, StageLabel)]
enum FirstStage {
        First,
}

#[derive(Clone, Hash, Debug, Eq, PartialEq, StageLabel)]
enum UpdateStage {
        PreUpdate,
        Update,
        PostUpdate,
}

#[derive(Clone, Hash, Debug, Eq, PartialEq, StageLabel)]
enum RenderStage {
        Render,
}

fn register_event<T: Resource>(world: &mut World, schedule: &mut Schedule) {
        let events = Events::<T>::from_world(world);
        world.insert_resource(events);
        schedule.add_system_to_stage(CoreStage::Cleanup, Events::<T>::update_system);
}

#[repr(C)]
#[derive(ShaderStruct)]
pub struct ShaderSettings {
        pub alt_normals: Vec2u,
        pub gamma_and_exposure: Vec2,
}

impl Default for ShaderSettings {
        fn default() -> Self {
                Self {
                        alt_normals: Vec2u::new(0, 0),
                        gamma_and_exposure: Vec2::new(2.2, 1.0),
                }
        }
}
