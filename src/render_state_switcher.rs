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

	pub fn is_new_state_available(&self) -> bool {
		self.is_state_new
	}

	pub fn read_new_render_state(&mut self, old_render_state: Option<Box<RenderState>>) -> Box<RenderState> {
		assert!(self.is_state_new);
		self.is_state_new = false;
		std::mem::replace(&mut self.switch_state, old_render_state).unwrap()
	}
}
