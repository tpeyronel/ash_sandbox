use std::rc::Rc;

use winit::{
        monitor::VideoMode,
        window::{Fullscreen, Window},
};

use crate::application::{CursorState, WindowMode};

pub struct WindowManager {
        window: Rc<Window>,
        fullscreen_video_mode: VideoMode,

        window_mode: WindowMode,
        cursor_state: CursorState,
        has_focus: bool,
}

impl WindowManager {
        pub fn new(
                window: Rc<Window>,
                fullscreen_video_mode: VideoMode,
                window_mode: WindowMode,
                cursor_state: CursorState,
        ) -> Self {
                Self::set_cursor_state_inner(&window, cursor_state);
                Self::set_window_mode_inner(&window, &fullscreen_video_mode, window_mode);

                Self {
                        window,
                        window_mode,
                        fullscreen_video_mode,
                        cursor_state,
                        has_focus: true,
                }
        }

        pub fn is_focused(&self) -> bool {
                self.has_focus
        }

        pub fn cursor_state(&self) -> CursorState {
                self.cursor_state
        }

        pub fn on_window_focused(&mut self, focused: bool) {
                self.has_focus = focused;

                if self.window_mode == WindowMode::Fullscreen {
                        let window_mode = if focused {
                                WindowMode::Fullscreen
                        } else {
                                WindowMode::Windowed
                        };

                        self.set_window_mode_silently(window_mode);
                }
        }

        pub fn set_cursor_state(&mut self, cursor_state: CursorState) {
                self.cursor_state = cursor_state;
                Self::set_cursor_state_inner(&self.window, cursor_state);
        }

        pub fn set_window_mode(&mut self, window_mode: WindowMode) {
                self.window_mode = window_mode;
                self.set_window_mode_silently(window_mode);
        }

        pub fn set_window_mode_silently(&mut self, window_mode: WindowMode) {
                Self::set_window_mode_inner(&self.window, &self.fullscreen_video_mode, window_mode);
        }

        fn set_cursor_state_inner(window: &Window, cursor_state: CursorState) {
                window.set_cursor_visible(cursor_state != CursorState::Hidden);
                window.set_cursor_grab(cursor_state == CursorState::Hidden).unwrap();
        }

        fn set_window_mode_inner(window: &Window, fullscreen_video_mode: &VideoMode, window_mode: WindowMode) {
                window.set_fullscreen(match window_mode {
                        WindowMode::Windowed => None,
                        WindowMode::Borderless => Some(Fullscreen::Borderless(None)),
                        WindowMode::Fullscreen => Some(Fullscreen::Exclusive(fullscreen_video_mode.clone())),
                });
        }
}
