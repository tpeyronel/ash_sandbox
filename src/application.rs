use std::{
        path::Path,
        rc::Rc,
        sync::{Arc, Mutex},
        time::Instant,
};

use slotmap::SlotMap;
use specs::{DispatcherBuilder, Entity, ReadExpect, WriteStorage};
use tps_counter::TPSCounter;

use crate::{
        actions::*,
        application_config::ApplicationConfig,
        asset_manager::*,
        constants::{FONT_SIZE, MAX_CONCURRENT_FRAMES, ROTATION_PER_SECOND},
        euler_angles::EulerAngles,
        hashmap::HashMap,
        input_manager::{
                ActionReceiver, InputBindingMap, InputManager, KeyBindingType, KeyCode, KeyState, MouseMotionType,
        },
        logic_thread::{
                LogicThread, LogicThreadCommand, LogicThreadMessage, LogicThreadSpawnParams, PlayerResource,
                TransformComponent,
        },
        my_glm::*,
        render_state_switcher::RenderStateSwitcher,
        renderer::{
                MeshInstance, MeshInstanceId, ModelInstance, ModelInstanceId, RenderState, Renderer, TransformManager,
        },
        vk::vk_renderer::VkRenderer,
        AnyResult,
};
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
        target_ticktime: f32,

        event_loop: Option<EventLoop<()>>,
        window: Rc<Window>,
        window_state: WindowState,
        window_thread_rx: std::sync::mpsc::Receiver<WindowThreadMessage>,
        logic_thread_tx: std::sync::mpsc::Sender<LogicThreadMessage>,
        imgui_context: ImguiContext,
        input_manager: InputManager,
        dispatch_actions: bool,
        action_receiver: ActionReceiver,
        asset_manager: Arc<AssetManager>,

        mesh_instances: SlotMap<MeshInstanceId, MeshInstance>,
        model_instances: SlotMap<ModelInstanceId, ModelInstance>,
        model_instances_index: HashMap<Entity, ModelInstanceId>,

        render_state_manager: RenderStateManager,
        renderer: VkRenderer,
        transform_manager: TransformManager,

        player_orien: EulerAngles,
        player_camera_enabled: bool,

        tps_counter: TPSCounter,
        frame_begin: Instant,
        delta_time: f32,

        player_transform: SharedValueSlave<TransformComponent>,
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
                        .with_min_inner_size(winit::dpi::PhysicalSize {
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

                asset_manager.register_shader_resource(
                        "matrices".to_string(),
                        ShaderResource {
                                elements: vec![
                                        ShaderResourceElement {
                                                element_type: ShaderResourceElementType::UniformBuffer,
                                                shader_stage_flags: ash::vk::ShaderStageFlags::VERTEX,
                                        },
                                        ShaderResourceElement {
                                                element_type: ShaderResourceElementType::UniformBufferDynamic,
                                                shader_stage_flags: ash::vk::ShaderStageFlags::VERTEX,
                                        },
                                ],
                        },
                );

                asset_manager.register_shader_resource(
                        "material-texture-sampler".to_string(),
                        ShaderResource {
                                elements: vec![
                                        ShaderResourceElement {
                                                element_type: ShaderResourceElementType::SampledImage,
                                                shader_stage_flags: ash::vk::ShaderStageFlags::FRAGMENT,
                                        },
                                        ShaderResourceElement {
                                                element_type: ShaderResourceElementType::Sampler,
                                                shader_stage_flags: ash::vk::ShaderStageFlags::FRAGMENT,
                                        },
                                ],
                        },
                );

                let _basic_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/basic_shader/basic_shader.yaml"))?;
                let _color_shader =
                        asset_manager.load_shader_from_yaml(Path::new("res/shader/color_shader/color_shader.yaml"))?;


                let _model_colt =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/new-colt/colt.gltf"))?;
                let _model_grass_plane =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/GrassPlane/GrassPlane.gltf"))?;
                let _model_sphere =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/sphere/sphere.gltf"))?;
                let _model_icosphere =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/icosphere/icosphere.gltf"))?;
                let _model_lit_icosphere = asset_manager
                        .import_gltf_file(std::path::Path::new("res/model/lit-icosphere/lit-icosphere.gltf"))?;


                let asset_manager = Arc::new(asset_manager);
                trace!("Initialized AssetManager");

                let render_state_switcher = Arc::new(Mutex::new(RenderStateSwitcher::new()));
                let render_state_manager = RenderStateManager::new(Arc::clone(&render_state_switcher));

                let renderer = VkRenderer::new(
                        Rc::clone(&window),
                        &mut imgui_context.context,
                        Arc::clone(&asset_manager),
                )?;

                let (logic_thread_tx, logic_thread_rx) = std::sync::mpsc::channel();
                let (window_thread_tx, window_thread_rx) = std::sync::mpsc::channel();

                let (player_transform_master, player_transform_slave) =
                        SharedValueMaster::new(TransformComponent::default());

                let systems: Vec<Box<dyn FnOnce(&mut DispatcherBuilder) + Send + Sync>> =
                        vec![Box::new(|builder: &mut DispatcherBuilder| {
                                builder.add(
                                        LoggerSystem {
                                                player_transform: player_transform_master,
                                        },
                                        "logger-system",
                                        &[],
                                )
                        })];

                let logic_thread_params = LogicThreadSpawnParams {
                        target_tps: config.tps,
                        logic_thread_rx,
                        window_thread_tx,
                        action_receiver: input_manager.create_action_receiver(),
                        asset_manager: Arc::clone(&asset_manager),
                        render_state_switcher,
                        systems,
                };

                let logic_thread = LogicThread::spawn(logic_thread_params);

                let action_receiver = input_manager.create_action_receiver();

                Ok(Self {
                        target_ticktime: 1.0 / config.tps as f32,
                        event_loop: Some(event_loop),
                        window,
                        window_state,
                        window_thread_rx,
                        logic_thread_tx,
                        imgui_context,
                        input_manager,
                        dispatch_actions,
                        action_receiver,
                        asset_manager,

                        mesh_instances: SlotMap::with_key(),
                        model_instances: SlotMap::with_key(),
                        model_instances_index: HashMap::new(),

                        render_state_manager,
                        renderer,
                        transform_manager: TransformManager::new(MAX_CONCURRENT_FRAMES),

                        player_orien: EulerAngles::new(0.0, 0.0, 0.0),
                        player_camera_enabled: false,

                        tps_counter: TPSCounter::new(5),
                        frame_begin: Instant::now(),
                        delta_time: 0.0,

                        player_transform: player_transform_slave,
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
                self.process_logic_thread_messages(control_flow);

                self.input_manager
                        .set_dispatch_actions(self.window_state.has_focus && self.dispatch_actions);

                for (action_id, strength) in self.action_receiver.receive_adjusted(self.delta_time) {
                        if !self.player_camera_enabled {
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

                self.logic_thread_tx
                        .send(LogicThreadMessage::SetPlayerOrien(self.player_orien.to_quat()))
                        .expect("Failed to send command to logic thread!");

                if let Some(render_state) = self.render_state_manager.get_render_state(self.target_ticktime) {
                        self.transform_manager.on_update();
                        self.update_instance_transforms(&render_state);

                        let imgui_ui = Self::build_imgui_ui(
                                &mut self.imgui_context,
                                &self.window,
                                &mut self.dispatch_actions,
                                &mut self.player_orien,
                                &mut self.player_transform,
                        )?;

                        self.renderer.draw(
                                &self.mesh_instances,
                                &self.model_instances,
                                &self.model_instances_index,
                                &self.transform_manager,
                                &render_state,
                                &self.player_orien.to_quat(),
                                imgui_ui.render(),
                        )?;
                }

                Ok(())
        }

        fn update_instance_transforms(&mut self, render_state: &RenderState) {
                let mut matrix_stack = MatrixStack::new();

                for (entity, (minstance_transform, minstance_model)) in &render_state.model_instances {
                        let minstance_id = match self.model_instances_index.get(entity) {
                                Some(&minstance_id) => minstance_id,
                                None => {
                                        let minstance_id = Self::create_model_instance(
                                                &self.asset_manager,
                                                &mut self.mesh_instances,
                                                &mut self.model_instances,
                                                minstance_model.0,
                                                *minstance_transform,
                                        );
                                        self.model_instances_index.insert(*entity, minstance_id);

                                        minstance_id
                                },
                        };

                        self.model_instances[minstance_id].transform = *minstance_transform;
                        let minstance = &self.model_instances[minstance_id];

                        Self::process_model_instance(
                                &mut matrix_stack,
                                &mut self.transform_manager,
                                &self.asset_manager,
                                &self.model_instances,
                                minstance,
                        );
                }
        }

        fn create_model_instance(
                asset_manager: &AssetManager,
                mesh_instances: &mut SlotMap<MeshInstanceId, MeshInstance>,
                model_instances: &mut SlotMap<ModelInstanceId, ModelInstance>,
                model_id: ModelId,
                transform: TransformComponent,
        ) -> ModelInstanceId {
                let model = &asset_manager.models()[model_id];

                let mut mesh_instance_ids = Vec::new();
                for &mesh_id in &model.meshes {
                        let mesh = &asset_manager.meshes()[mesh_id];
                        let mesh_instance_id = mesh_instances.insert(MeshInstance {
                                mesh_id,
                                transform: TransformComponent::default(),
                        });
                        mesh_instance_ids.push(mesh_instance_id);
                }

                let mut child_model_instance_ids = Vec::new();
                for &child_model_id in &model.children {
                        child_model_instance_ids.push(Self::create_model_instance(
                                asset_manager,
                                mesh_instances,
                                model_instances,
                                child_model_id,
                                TransformComponent::default(),
                        ));
                }

                let model_instance = ModelInstance {
                        model_id,
                        mesh_instances: mesh_instance_ids,
                        children: child_model_instance_ids,
                        transform,
                };

                model_instances.insert(model_instance)
        }

        fn process_model_instance(
                matrix_stack: &mut MatrixStack,
                transform_manager: &mut TransformManager,
                asset_manager: &AssetManager,
                model_instances: &SlotMap<ModelInstanceId, ModelInstance>,
                model_instance: &ModelInstance,
        ) {
                matrix_stack.push(model_instance.transform.to_matrix());
                let transform = matrix_stack.push(asset_manager.models()[model_instance.model_id].base_transform);

                for &mesh_instance_id in &model_instance.mesh_instances {
                        transform_manager.set_transform(mesh_instance_id, &transform);
                }

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

        fn process_logic_thread_messages(&mut self, control_flow: &mut ControlFlow) {
                for message in self.window_thread_rx.try_iter() {
                        match message {
                                WindowThreadMessage::Command(command) => Self::process_command(
                                        command,
                                        control_flow,
                                        &self.window,
                                        &mut self.imgui_context.context.io_mut(),
                                        &mut self.window_state,
                                        &mut self.player_camera_enabled,
                                ),
                        }
                }
        }

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

                let _ = self
                        .logic_thread_tx
                        .send(LogicThreadMessage::Command(LogicThreadCommand::Exit));
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
