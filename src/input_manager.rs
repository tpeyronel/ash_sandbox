use std::collections::HashMap;

#[allow(unused_imports)]
use log::info;
use winit::event::{DeviceEvent, KeyboardInput, ModifiersState};

pub type KeyCode = winit::event::VirtualKeyCode;
pub type KeyState = winit::event::ElementState;

const MAX_KEY_CODE: usize = KeyCode::Cut as usize;


#[derive(Debug, Clone)]
pub struct UserAction {
	pub action_id: ActionId,
	pub action_type: ActionType,
	pub input_type: InputValueType,
}

#[derive(Debug, Clone)]
pub struct ActionBinding {
	pub action_id: ActionId,
	pub action_type: ActionType,
}

/* #[derive(Debug, Clone)]
pub struct ActionId(pub String); */

pub type ActionId = String;

#[derive(Debug, Clone)]
pub enum ActionType {
	Instantaneous,
	Prolonged{ stage: ProlongedActionTypeStage },
}

#[derive(Debug, Clone)]
pub enum ProlongedActionTypeStage {
	Begin,
	End,
}

#[derive(Debug, Clone, Copy)]
pub enum InputValueType {
	Discrete,
	Continuous(f32),
}

pub type ActionIdRx = bus::BusReader<UserAction>;
type ActionIdTx = bus::Bus<UserAction>;

pub struct InputManager {
	key_states: [KeyState; MAX_KEY_CODE],
	modifiers_state: ModifiersState,

	key_press_bindings: Vec<HashMap<KeyCode, ActionBinding>>,
	key_release_bindings: Vec<HashMap<KeyCode, ActionBinding>>,

	action_id_tx: ActionIdTx,
}

impl InputManager {
	pub fn new() -> Self {
		let action_id_tx = bus::Bus::new(50);

		Self {
			key_states: [KeyState::Released; MAX_KEY_CODE],
			modifiers_state: ModifiersState::empty(),
			key_press_bindings: Vec::new(),
			key_release_bindings: Vec::new(),
			action_id_tx,
		}
	}

	pub fn create_rx(&mut self) -> ActionIdRx {
		self.action_id_tx.add_rx()
	}

	pub fn get_key_states(&self) -> &[KeyState; MAX_KEY_CODE] {
		&self.key_states
	}

	pub fn push_key_press_input_map(&mut self, map: HashMap<KeyCode, ActionBinding>) {
		self.key_press_bindings.push(map);
	}

	pub fn push_key_release_input_map(&mut self, map: HashMap<KeyCode, ActionBinding>) {
		self.key_release_bindings.push(map);
	}

	pub fn on_device_event(&mut self, device_event: &DeviceEvent) {
		match device_event {
			DeviceEvent::MouseMotion { delta } => (),
			DeviceEvent::MouseWheel { delta } => (),
			DeviceEvent::Motion { axis, value } => (),
			DeviceEvent::Button { button, state } => (),
			DeviceEvent::Key(kinput) => self.on_keyboard_input(kinput),
			_ => (),
		}
	}

	fn on_keyboard_input(&mut self, kinput: &KeyboardInput) {
		let key_code = match kinput.virtual_keycode {
			Some(key_code) => key_code,
			None => return,
		};
		let key_state = kinput.state;

		self.key_states[key_code as usize] = key_state;

		let action_binding = match key_state {
			KeyState::Pressed => self.key_press_bindings.iter().rev().find_map(|b| b.get(&key_code)),
			KeyState::Released => self.key_release_bindings.iter().rev().find_map(|b| b.get(&key_code)),
		}.cloned();
		let action_binding = match action_binding {
			Some(action_binding) => action_binding,
			None => return,
		};

		self.broadcast_action(
			action_binding.action_id,
			action_binding.action_type,
			InputValueType::Discrete,
		);
	}

	fn broadcast_action(&mut self, action_id: ActionId, action_type: ActionType, input_type: InputValueType) {
		let action = UserAction {
			action_id,
			action_type,
			input_type,
		};

		self.action_id_tx.broadcast(action);
	}
}
