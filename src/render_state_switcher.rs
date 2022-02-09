use crate::renderer::RenderState;

pub struct RenderStateSwitcher {
        switch_state: Option<Box<RenderState>>,
        is_state_new: bool,
}

impl RenderStateSwitcher {
        pub fn new() -> Self {
                Self {
                        switch_state: None,
                        is_state_new: false,
                }
        }

        pub fn write_render_state(&mut self, render_state: Box<RenderState>) -> Option<Box<RenderState>> {
                self.is_state_new = true;
                self.switch_state.replace(render_state)
        }

        pub fn try_exchange(&mut self, old_render_state: &mut Option<Box<RenderState>>) -> Option<Box<RenderState>> {
                if self.is_state_new {
                        self.is_state_new = false;
                        Some(std::mem::replace(&mut self.switch_state, old_render_state.take()).unwrap())
                } else {
                        None
                }
        }
}
