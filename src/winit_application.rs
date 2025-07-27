use winit::{
        event::WindowEvent,
        event_loop::{ControlFlow, EventLoop},
};

use crate::{application::Application, AnyResult};

pub struct WinitApplication {
        app: Option<Application>,
}

impl WinitApplication {
        pub fn new() -> Self {
                Self { app: None }
        }

        pub fn run(mut self) -> AnyResult<()> {
                let event_loop = EventLoop::new()?;
                event_loop.set_control_flow(ControlFlow::Poll);
                event_loop.run_app(&mut self)?;

                Ok(())
        }

        fn on_winit_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: winit::event::Event<()>) {
                let Some(app) = &mut self.app else { return };

                let mut quit = false;

                app.on_winit_event(event, &mut quit)
                        .expect("Error ocurred in render loop");

                if quit {
                        event_loop.exit();
                }
        }
}

impl winit::application::ApplicationHandler for WinitApplication {
        fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
                match self.app {
                        Some(_) => (),
                        None => {
                                self.app =
                                        Some(Application::new(event_loop)
                                                .expect("Error ocurred initializing application"));
                        },
                }

                self.on_winit_event(event_loop, winit::event::Event::Resumed);
        }

        fn user_event(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, event: ()) {
                self.on_winit_event(event_loop, winit::event::Event::UserEvent(event));
        }

        fn window_event(
                &mut self,
                event_loop: &winit::event_loop::ActiveEventLoop,
                window_id: winit::window::WindowId,
                event: WindowEvent,
        ) {
                self.on_winit_event(event_loop, winit::event::Event::WindowEvent { window_id, event });
        }

        fn new_events(&mut self, event_loop: &winit::event_loop::ActiveEventLoop, cause: winit::event::StartCause) {
                self.on_winit_event(event_loop, winit::event::Event::NewEvents(cause));
        }

        fn device_event(
                &mut self,
                event_loop: &winit::event_loop::ActiveEventLoop,
                device_id: winit::event::DeviceId,
                event: winit::event::DeviceEvent,
        ) {
                self.on_winit_event(event_loop, winit::event::Event::DeviceEvent { device_id, event });
        }

        fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
                self.on_winit_event(event_loop, winit::event::Event::AboutToWait);
        }

        fn suspended(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
                self.on_winit_event(event_loop, winit::event::Event::Suspended);
        }

        fn exiting(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
                self.on_winit_event(event_loop, winit::event::Event::LoopExiting);
        }

        fn memory_warning(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
                self.on_winit_event(event_loop, winit::event::Event::MemoryWarning);
        }
}
