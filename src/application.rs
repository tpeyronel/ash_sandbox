use std::{
        error::Error,
        rc::Rc,
        time::{Duration, Instant},
};

use fps_counter::FPSCounter;

use crate::{
        asset_manager::*,
        camera::Camera,
        input_manager::{InputManager, InputMessage},
        my_glm::*,
        renderer::Renderer,
        vk::vk_renderer::VkRenderer,
};
use log::{info, trace};
use winit::{
        dpi::PhysicalSize,
        event::{DeviceEvent, Event, ModifiersState, StartCause, VirtualKeyCode, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Fullscreen, Window, WindowBuilder},
};

const FONT_SIZE: f32 = 13.0;

#[allow(dead_code)]
pub struct Application {
        event_loop: Option<EventLoop<()>>,
        window: Rc<Window>,
        window_state: WindowState,
        imgui_state: ImGuiState,
        input_manager: InputManager,
        asset_manager: AssetManager,
        renderer: VkRenderer,
        camera: Camera,

        fps_counter: FPSCounter,
        prev_frame_begin: Instant,
        frame_begin: Instant,
        dtime: Duration,
}

impl Application {
        pub fn new(_fullscreen: bool) -> Result<Self, Box<dyn Error>> {
                let event_loop = EventLoop::new();
                let window = Rc::new(WindowBuilder::new()
                        //.with_fullscreen(Some(fullscreen_mode.clone()))
                        //.with_fullscreen(Some(Fullscreen::Borderless(None)))
                        .with_fullscreen(None)
                        .with_visible(false)
                        .with_always_on_top(false)
                        .with_min_inner_size(winit::dpi::PhysicalSize {
                                width: 240,
                                height: 240,
                        })
                        .build(&event_loop)?);
                trace!("Created window");

                let fullscreen_mode =
                        Fullscreen::Exclusive(event_loop.primary_monitor().unwrap().video_modes().next().unwrap());

                let window_state = WindowState {
                        fullscreen_mode,
                        cursor_state: CursorState::Normal,
                        focused: true,
                };

                let mut imgui_state = Self::init_imgui(&window);
                trace!("Initialized ImGui");

                let mut input_manager = InputManager::new();
                input_manager.on_key_release(VirtualKeyCode::Escape, InputMessage::Exit);
                input_manager.on_key_release(VirtualKeyCode::T, InputMessage::ToggleCursor);
                input_manager.on_key_release(VirtualKeyCode::F11, InputMessage::ToggleFullscreen);


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
                let _model_colt = asset_manager.import_gltf_file(std::path::Path::new("res/model/Colt/Colt.gltf"))?;



                let renderer = VkRenderer::new(&window, &mut imgui_state.context, &asset_manager)?;
                let camera = Camera::new(
                        &Vec3::new(0.0, 0.0, -2.0),
                        0.0,
                        0.0,
                        0.0,
                        90.0f32.to_radians(),
                        5.0,
                        window.inner_size().width,
                        window.inner_size().height,
                        0.1,
                        100.0,
                );

                window.set_visible(true);
                window.set_cursor_visible(window_state.cursor_state != CursorState::Hidden);
                window.set_cursor_grab(window_state.cursor_state == CursorState::Hidden)
                        .expect("Error ocurred trying to grab cursor!");
                imgui_state.context.io_mut().config_flags.set(
                        imgui::ConfigFlags::NO_MOUSE,
                        window_state.cursor_state == CursorState::Hidden,
                );


                Ok(Self {
                        event_loop: Some(event_loop),
                        window,
                        window_state,
                        imgui_state,
                        input_manager,
                        asset_manager,
                        renderer,
                        camera,

                        fps_counter: FPSCounter::new(
                                Duration::from_millis(250),
                                Box::new(|fps| {
                                        info!("FPS: {}", fps);
                                }),
                        ),
                        prev_frame_begin: Instant::now(),
                        frame_begin: Instant::now(),
                        dtime: Duration::from_nanos(0),
                })
        }

        pub fn run(mut self) -> ! {
                self.event_loop.take().unwrap().run(move |event, _, control_flow| {
                        *control_flow = ControlFlow::Poll;

                        self.imgui_state
                                .platform
                                .handle_event(self.imgui_state.context.io_mut(), &self.window, &event);

                        match event {
                                Event::NewEvents(start_cause) => self.on_new_events(&start_cause),
                                Event::DeviceEvent { event, .. } => {
                                        if self.window_state.focused {
                                                self.on_device_event(&event)
                                        }
                                }
                                Event::WindowEvent { window_id, event } => {
                                        if self.window.id() == window_id {
                                                self.on_window_event(&event, control_flow);
                                        }
                                }
                                Event::MainEventsCleared => self.update(control_flow),
                                _ => (),
                        }
                });
        }

        fn init_imgui(window: &Window) -> ImGuiState {
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

                ImGuiState { context, platform }
        }

        fn on_new_events(&mut self, _start_cause: &StartCause) {
                self.prev_frame_begin = self.frame_begin;
                self.frame_begin = Instant::now();
                self.dtime = self.frame_begin - self.prev_frame_begin;

                self.imgui_state.context.io_mut().update_delta_time(self.dtime);

                self.input_manager.update();
        }

        fn on_device_event(&mut self, devent: &DeviceEvent) {
                match *devent {
                        DeviceEvent::MouseMotion { delta: (dx, dy) } => {
                                if self.window_state.cursor_state == CursorState::Hidden {
                                        if self.input_manager.all_modifiers(ModifiersState::ALT) {
                                                self.camera.rotate_euler_angles(-dy as f32, 0.0, dx as f32);
                                        } else {
                                                self.camera.rotate_euler_angles(-dy as f32, dx as f32, 0.0);
                                        }
                                }
                        }
                        _ => (),
                };
        }

        fn on_window_event(&mut self, wevent: &WindowEvent, control_flow: &mut ControlFlow) {
                match *wevent {
                        WindowEvent::ModifiersChanged(modifiers_state) => {
                                self.input_manager.on_modifiers_changed(modifiers_state)
                        }
                        WindowEvent::Focused(focused) => self.window_state.focused = focused,
                        WindowEvent::Resized(PhysicalSize { width, height }) => {
                                self.camera.on_window_resize(width, height);
                                self.renderer.on_window_resize(width, height);
                        }
                        WindowEvent::CloseRequested => {
                                *control_flow = ControlFlow::Exit;
                        }
                        WindowEvent::KeyboardInput { input, .. } => {
                                self.input_manager.on_keyboard_input(&input);
                        }
                        _ => {}
                };
        }

        fn update(&mut self, control_flow: &mut ControlFlow) {
                self.fps_counter.tick();

                for input_message in self.input_manager.next_message() {
                        match input_message {
                                InputMessage::Exit => *control_flow = ControlFlow::Exit,
                                InputMessage::ToggleCursor => {
                                        let new_cursor_state = match self.window_state.cursor_state {
                                                CursorState::Normal => CursorState::Hidden,
                                                CursorState::Hidden => CursorState::Normal,
                                        };

                                        Self::set_cursor_state(
                                                &mut self.window_state,
                                                &mut self.window,
                                                self.imgui_state.context.io_mut(),
                                                new_cursor_state,
                                        );
                                }
                                InputMessage::ToggleFullscreen => {
                                        match self.window.fullscreen() {
                                                Some(_) => self.window.set_fullscreen(None),
                                                None => self.window.set_fullscreen(Some(self
                                                        .window_state
                                                        .fullscreen_mode
                                                        .clone())),
                                        };
                                }
                                _ => (),
                        }
                }

                let input = &self.input_manager;

                let mut desired_dir = Vec3::new(0.0, 0.0, 0.0);

                if input.key_pressed(VirtualKeyCode::W) {
                        desired_dir.z += 1.0;
                }
                if input.key_pressed(VirtualKeyCode::S) {
                        desired_dir.z -= 1.0;
                }
                if input.key_pressed(VirtualKeyCode::D) {
                        desired_dir.x += 1.0;
                }
                if input.key_pressed(VirtualKeyCode::A) {
                        desired_dir.x -= 1.0;
                }
                if input.key_pressed(VirtualKeyCode::Space) {
                        desired_dir.y += 1.0;
                }
                if input.key_pressed(VirtualKeyCode::LShift) {
                        desired_dir.y -= 1.0;
                }

                let move_speed = 0.0075f32;

                if desired_dir.norm() > f32::EPSILON {
                        let move_dir = *self.camera.get_hor_orientation() * desired_dir.normalize() * move_speed;
                        self.camera.translate(&move_dir);
                }

                self.imgui_state
                        .platform
                        .prepare_frame(self.imgui_state.context.io_mut(), &self.window)
                        .expect("Failed to prepare frame");
                let ui = self.imgui_state.context.frame();
                let cam = &self.camera;

                imgui::Window::new(im_str!("Hello world"))
                        .size([300.0, 100.0], imgui::Condition::FirstUseEver)
                        .build(&ui, || {
                                ui.text(im_str!("Hello world!"));
                                ui.text(im_str!("こんにちは世界！"));
                                ui.text(im_str!("This...is...imgui-rs!"));
                                ui.separator();
                                let mouse_pos = ui.io().mouse_pos;
                                ui.text(format!("Mouse Position: ({:.1},{:.1})", mouse_pos[0], mouse_pos[1]));
                                ui.text(format!(
                                        "Pitch: {:.1} Yaw: {:.1}, Roll: {:.1}",
                                        cam.get_pitch().to_degrees(),
                                        cam.get_yaw().to_degrees(),
                                        cam.get_roll().to_degrees()
                                ));
                        });

                let mut _opened: bool = false;
                ui.show_demo_window(&mut _opened);

                let imgui_draw_data = ui.render();

                self.renderer
                        .draw(&self.camera, imgui_draw_data)
                        .expect("Error occurred while drawing");
        }

        fn set_cursor_state(
                window_state: &mut WindowState,
                window: &mut Rc<Window>,
                imgui_io: &mut imgui::Io,
                new_state: CursorState,
        ) {
                window_state.cursor_state = new_state;
                window.set_cursor_visible(new_state != CursorState::Hidden);
                window.set_cursor_grab(new_state == CursorState::Hidden).unwrap();
                imgui_io.config_flags
                        .set(imgui::ConfigFlags::NO_MOUSE, new_state == CursorState::Hidden);
        }
}

struct WindowState {
        fullscreen_mode: Fullscreen,
        cursor_state: CursorState,
        focused: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum CursorState {
        Normal,
        Hidden,
}

struct ImGuiState {
        context: imgui::Context,
        platform: imgui_winit_support::WinitPlatform,
}
