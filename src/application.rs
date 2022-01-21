use std::{
        error::Error,
        path::Path,
        rc::Rc,
        sync::{Arc, Mutex},
        time::Instant,
};

use tps_counter::TPSCounter;

use crate::{
        actions::*,
        application_config::ApplicationConfig,
        asset_manager::*,
        constants::{FONT_SIZE, PIXELS_PER_TURN},
        euler_angles::EulerAngles,
        input_manager::{
                ActionReceiver, InputBindingMap, InputManager, KeyBindingType, KeyCode, KeyState, MouseMotionType,
        },
        logic_thread::{LogicThread, LogicThreadCommand, LogicThreadMessage, LogicThreadSpawnParams},
        my_glm::*,
        render_state_switcher::RenderStateSwitcher,
        renderer::Renderer,
        vk::vk_renderer::VkRenderer,
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
        event_loop: Option<EventLoop<()>>,
        window: Rc<Window>,
        window_state: WindowState,
        window_thread_rx: std::sync::mpsc::Receiver<WindowThreadMessage>,
        logic_thread_tx: std::sync::mpsc::Sender<LogicThreadMessage>,
        imgui_context: ImguiContext,
        input_manager: InputManager,
        action_receiver: ActionReceiver,
        asset_manager: Arc<AssetManager>,

        renderer: VkRenderer,

        player_orien: EulerAngles,
        player_camera_enabled: bool,

        tps_counter: TPSCounter,
        frame_begin: Instant,
}

impl Application {
        pub fn new() -> Result<Self, Box<dyn Error>> {
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

                let mut imgui_state = Self::init_imgui(&window);

                let window_state = WindowState::new(
                        &window,
                        imgui_state.context.io_mut(),
                        WindowMode::Windowed,
                        fullscreen_video_mode,
                        CursorState::Normal,
                );

                trace!("Initialized ImGui");

                let mut input_manager = InputManager::new();

                // let pollable_actions = input_manager.clone_continuous_actions_state();

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

                input_map.bind_mouse_motion(YAW_NEGATIVE, MouseMotionType::PositiveX, None);
                input_map.bind_mouse_motion(YAW_POSITIVE, MouseMotionType::NegativeX, None);
                input_map.bind_mouse_motion(PITCH_POSITIVE, MouseMotionType::PositiveY, None);
                input_map.bind_mouse_motion(PITCH_NEGATIVE, MouseMotionType::NegativeY, None);
                input_map.bind_key(ROLL_NEGATIVE, KeyCode::Right, KeyBindingType::Continuous);
                input_map.bind_key(ROLL_POSITIVE, KeyCode::Left, KeyBindingType::Continuous);

                input_manager.push_input_binding_map(input_map);

                trace!("Initialized InputManager");

                let dsampler = Sampler {
                        name: Some(String::from("Default Sampler")),
                        mag_filter: MagFilter::Linear,
                        min_filter: MinFilter::LinearMipmapLinear,
                        wrap_s: WrappingMode::Repeat,
                        wrap_t: WrappingMode::Repeat,
                };

                let dmaterial = Material {
                        name: Some(String::from("Default Material")),
                        base_color_factor: Vec4::new(0.8, 0.8, 0.8, 1.0),
                        metallic_factor: 0.0,
                        roughness_factor: 1.0,
                        base_color_texture: None,
                        metallic_roughness_texture: None,
                        normal_texture: None,
                        occlusion_texture: None,
                        emissive_texture: None,
                };

                let mut asset_manager = AssetManager::new(dsampler, dmaterial);
                let _model_colt =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/new-colt/colt.gltf"))?;
                let _model_grass_plane =
                        asset_manager.import_gltf_file(std::path::Path::new("res/model/GrassPlane/GrassPlane.gltf"))?;
                let asset_manager = Arc::new(asset_manager);
                trace!("Initialized AssetManager");

                let render_state_switcher = Arc::new(Mutex::new(RenderStateSwitcher::new()));
                let renderer = VkRenderer::new(
                        config.tps,
                        Rc::clone(&window),
                        &mut imgui_state.context,
                        Arc::clone(&asset_manager),
                        Arc::clone(&render_state_switcher),
                )?;

                let (logic_thread_tx, logic_thread_rx) = std::sync::mpsc::channel();
                let (window_thread_tx, window_thread_rx) = std::sync::mpsc::channel();

                let logic_thread_params = LogicThreadSpawnParams {
                        target_tps: config.tps,
                        logic_thread_rx,
                        window_thread_tx,
                        action_receiver: input_manager.create_action_receiver(),
                        asset_manager: Arc::clone(&asset_manager),
                        render_state_switcher,
                };

                let logic_thread = LogicThread::spawn(logic_thread_params);

                let action_receiver = input_manager.create_action_receiver();

                Ok(Self {
                        event_loop: Some(event_loop),
                        window,
                        window_state,
                        window_thread_rx,
                        logic_thread_tx,
                        imgui_context: imgui_state,
                        input_manager,
                        action_receiver,
                        asset_manager,
                        renderer,

                        player_orien: EulerAngles::new(0.0, 0.0, 0.0),
                        player_camera_enabled: false,

                        tps_counter: TPSCounter::new(5),
                        frame_begin: Instant::now(),
                })
        }

        pub fn run(mut self) -> ! {
                self.window.set_visible(true);

                self.event_loop.take().unwrap().run(move |event, _, control_flow| {
                        *control_flow = ControlFlow::Poll;

                        match self.on_winit_event(event, control_flow) {
                                Ok(_) => (),
                                Err(err) => error!("Error ocurred in render loop: {}", err),
                        }
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
        ) -> Result<(), Box<dyn Error>> {
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

                self.imgui_context.context.io_mut().update_delta_time(delta_time);
        }

        fn on_window_event(&mut self, window_event: WindowEvent, control_flow: &mut ControlFlow) {
                match window_event {
                        /* WindowEvent::ModifiersChanged(modifiers_state) => {
                                self.input_manager.on_modifiers_changed(modifiers_state)
                        } */
                        WindowEvent::Focused(focused) => {
                                self.window_state.has_focus = focused;
                                self.input_manager.on_window_focused(focused);
                        },
                        WindowEvent::Resized(new_size) => {
                                self.renderer.on_window_resize(new_size.width, new_size.height);
                        },
                        WindowEvent::CloseRequested => {
                                *control_flow = ControlFlow::Exit;
                        },
                        /* WindowEvent::KeyboardInput { input, .. } => {
                                self.input_manager.on_keyboard_input(&input);
                        } */
                        _ => {},
                };
        }

        fn update(&mut self, control_flow: &mut ControlFlow) -> Result<(), Box<dyn Error>> {
                self.process_logic_thread_messages(control_flow);

                for (action_id, strength) in self.action_receiver.receive() {
                        if !self.player_camera_enabled {
                                continue;
                        }

                        match action_id {
                                YAW_POSITIVE => self.player_orien.yaw_by(strength.0 / PIXELS_PER_TURN),
                                YAW_NEGATIVE => self.player_orien.yaw_by(-strength.0 / PIXELS_PER_TURN),
                                PITCH_POSITIVE => self.player_orien.pitch_by(strength.0 / PIXELS_PER_TURN),
                                PITCH_NEGATIVE => self.player_orien.pitch_by(-strength.0 / PIXELS_PER_TURN),
                                ROLL_POSITIVE => self.player_orien.roll_by(strength.0 / PIXELS_PER_TURN),
                                ROLL_NEGATIVE => self.player_orien.roll_by(-strength.0 / PIXELS_PER_TURN),
                                _ => continue,
                        }

                        self.logic_thread_tx
                                .send(LogicThreadMessage::SetPlayerOrien(self.player_orien.to_quat()))
                                .expect("Failed to send command to logic thread!");
                }

                // self.tps_counter.tick_and_map(|tps| info!("FPS: {:.2}", tps));

                let imgui_ui = Self::build_imgui_ui(&mut self.imgui_context, &self.window, self.player_orien.clone());

                self.renderer.draw(&self.player_orien.to_quat(), imgui_ui?.render())
        }

        fn build_imgui_ui<'a>(
                imgui_state: &'a mut ImguiContext,
                window: &winit::window::Window,
                player_orien: EulerAngles,
        ) -> Result<imgui::Ui<'a>, winit::error::ExternalError> {
                imgui_state
                        .platform
                        .prepare_frame(imgui_state.context.io_mut(), window)?;

                let ui = imgui_state.context.frame();

                imgui::Window::new("Hello world")
                        .size([300.0, 100.0], imgui::Condition::FirstUseEver)
                        .build(&ui, || {
                                let mouse_pos = ui.io().mouse_pos;
                                ui.text(format!("Mouse Position: ({:.1},{:.1})", mouse_pos[0], mouse_pos[1]));
                                ui.separator();
                                ui.text(format!(
                                        "Pitch: {:.1}, Yaw: {:.1}, Roll: {:.1}",
                                        player_orien.pitch().to_degrees(),
                                        player_orien.yaw().to_degrees(),
                                        player_orien.roll().to_degrees(),
                                ));
                        });

                ui.show_demo_window(&mut false);

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

        pub fn set_cursor_state(&mut self, window: &Window, imgui_io: &mut imgui::Io, cursor_state: CursorState) {
                self.cursor_state = cursor_state;
                Self::set_cursor_state_inner(window, imgui_io, cursor_state);
        }

        pub fn set_window_mode(&mut self, window: &Window, window_mode: WindowMode) {
                self.window_mode = window_mode;
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
