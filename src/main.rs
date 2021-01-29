mod image;
mod my_glm;
mod renderer;
mod timer;
mod vertex;
mod vk_buffer;
mod vk_command_buffer;
mod vk_context;
mod vk_image;
mod vk_renderer;
mod vk_swapchain;
mod vk_wrapper;
mod vkma_error;

#[macro_use]
extern crate const_cstr;
#[macro_use]
extern crate imgui;
extern crate nalgebra as na;
extern crate nalgebra_glm as glm;
extern crate vk_mem as vma;

use std::{
        error::Error,
        io::Write,
        sync::Arc,
        time::{Duration, Instant},
};

use chrono::Local;
use env_logger::Env;
use fps_counter::FPSCounter;
use log::{info, trace, warn};
use winit::{
        event::{Event, VirtualKeyCode, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Fullscreen, WindowBuilder},
};

use crate::{renderer::Renderer, vk_renderer::VkRenderer};

fn main() -> Result<(), Box<dyn Error>> {
        env_logger::Builder::from_env(Env::default().default_filter_or("trace"))
                .format(|buf, record| {
                        writeln!(
                                buf,
                                "[{} {}] {}",
                                Local::now().time().format("%H:%M:%S").to_string(),
                                record.level(),
                                record.args()
                        )
                })
                .init();

        let event_loop = EventLoop::new();

        let fullscreen_mode =
                Fullscreen::Exclusive(event_loop.primary_monitor().unwrap().video_modes().next().unwrap());

        let window = Arc::new(
                WindowBuilder::new()
                        .with_fullscreen(Some(Fullscreen::Borderless(None)))
                        .with_fullscreen(Some(fullscreen_mode.clone()))
                        .with_fullscreen(None)
                        .with_visible(false)
                        .with_always_on_top(false)
                        .with_min_inner_size(winit::dpi::PhysicalSize {
                                width:  240,
                                height: 240,
                        })
                        .build(&event_loop)?,
        );
        trace!("Created window");

        let mut imgui_c = imgui::Context::create();

        let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui_c);

        let hidpi_factor = platform.hidpi_factor();
        let font_size = (13.0 * hidpi_factor) as f32;
        imgui_c.fonts().add_font(&[
                imgui::FontSource::DefaultFontData {
                        config: Some(imgui::FontConfig {
                                size_pixels: font_size,
                                ..imgui::FontConfig::default()
                        }),
                },
                imgui::FontSource::TtfData {
                        data:        include_bytes!("../res/font/FiraCode-Regular.ttf"),
                        size_pixels: font_size,
                        config:      Some(imgui::FontConfig {
                                rasterizer_multiply: 1.75,
                                glyph_ranges: imgui::FontGlyphRanges::japanese(),
                                ..imgui::FontConfig::default()
                        }),
                },
        ]);
        imgui_c.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;
        platform.attach_window(imgui_c.io_mut(), &window, imgui_winit_support::HiDpiMode::Rounded);

        let mut renderer = VkRenderer::new(&window, &mut imgui_c)?;

        let mut fps_ctr = FPSCounter::new();
        let mut last_print_fps = Instant::now();
        let mut last_frame = Instant::now();

        window.set_visible(true);
        event_loop.run(move |event, _, control_flow| {
                *control_flow = ControlFlow::Poll;

                platform.handle_event(imgui_c.io_mut(), &window, &event);

                match event {
                        Event::NewEvents(_) => {
                                let now = Instant::now();
                                imgui_c.io_mut().update_delta_time(now - last_frame);
                                last_frame = now;
                        }
                        Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                                WindowEvent::Resized(size) => {
                                        renderer.on_window_resize(size.width, size.height);
                                }
                                WindowEvent::CloseRequested => {
                                        *control_flow = ControlFlow::Exit;
                                }
                                WindowEvent::KeyboardInput { input, .. } => {
                                        if let Some(virtual_keycode) = input.virtual_keycode {
                                                match virtual_keycode {
                                                        VirtualKeyCode::Escape => *control_flow = ControlFlow::Exit,
                                                        VirtualKeyCode::F11 if input.state == winit::event::ElementState::Released => {
                                                                match window.fullscreen() {
                                                                        Some(_) => window.set_fullscreen(None),
                                                                        None => window.set_fullscreen(Some(
                                                                                fullscreen_mode.clone(),
                                                                        )),
                                                                };
                                                        }
                                                        _ => {}
                                                };
                                        }
                                }
                                _ => {}
                        },
                        Event::MainEventsCleared => {
                                let fps = fps_ctr.tick();
                                let now = Instant::now();
                                let time_since_last_print_fps = now - last_print_fps;

                                let print_interval = Duration::from_millis(250);
                                if time_since_last_print_fps > print_interval {
                                        last_print_fps += print_interval;
                                        info!("FPS: {}", fps);
                                }

                                platform.prepare_frame(imgui_c.io_mut(), &window)
                                        .expect("Failed to prepare frame");
                                let ui = imgui_c.frame();

                                imgui::Window::new(im_str!("Hello world"))
                                        .size([300.0, 100.0], imgui::Condition::FirstUseEver)
                                        .build(&ui, || {
                                                ui.text(im_str!("Hello world!"));
                                                ui.text(im_str!("こんにちは世界！"));
                                                ui.text(im_str!("This...is...imgui-rs!"));
                                                ui.separator();
                                                let mouse_pos = ui.io().mouse_pos;
                                                ui.text(format!(
                                                        "Mouse Position: ({:.1},{:.1})",
                                                        mouse_pos[0], mouse_pos[1]
                                                ));
                                        });

                                let mut opened: bool = false;
                                ui.show_demo_window(&mut opened);

                                let imgui_draw_data = ui.render();

                                renderer.draw(imgui_draw_data).expect("Error occurred while drawing");
                        }
                        _ => (),
                }
        });
}
