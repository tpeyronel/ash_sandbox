use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use enum_map::EnumMap;
#[allow(unused_imports)]
use log::info;
use winit::event::{DeviceEvent, KeyboardInput};

pub type KeyCode = winit::event::VirtualKeyCode;
pub type KeyState = winit::event::ElementState;

#[derive(Debug, Clone)]
pub struct ActionEvent {
	pub action_id: ActionId,
	pub strength: ActionStrength,
}

pub type ActionId = String;
#[derive(Debug, Clone, Copy)]
pub struct ActionStrength(pub f32);

const KEY_ACTION_STRENGTH: ActionStrength = ActionStrength(1.0);

pub type ActionIdRx = std::sync::mpsc::Receiver<ActionEvent>;
type ActionIdTx = std::sync::mpsc::Sender<ActionEvent>;

pub struct InputManager {
	window_focused: bool,
	keyboard_state: KeyboardState,
	// modifiers_state: ModifiersState,
	binding_map: InputBindingMap,
	input_state: Arc<Mutex<InputState>>,
}

impl InputManager {
	pub fn new() -> Self {
		Self {
			window_focused: true,
			keyboard_state: KeyboardState::new(),
			binding_map: InputBindingMap::new(),
			input_state: Arc::new(Mutex::new(InputState { values: HashMap::new() })),
		}
	}

	pub fn get_input_state(&mut self) -> Arc<Mutex<InputState>> {
		Arc::clone(&self.input_state)
	}

	pub fn push_input_binding_map(&mut self, map: InputBindingMap) {
		self.binding_map = map;
	}

	pub fn on_window_focused(&mut self, focused: bool) {
		self.window_focused = focused;

		for key_code in self.keyboard_state.iter_presed() {
			let KeyBinding {
				action_id,
				key_binding_type,
			} = match self.binding_map.get_key_binding(key_code) {
				Some(b) => b,
				None => continue,
			};

			match key_binding_type {
				KeyBindingType::Extended => {
					let mut input_state = self.input_state.lock().unwrap();

					if !focused {
						input_state.reset(action_id);
					} else {
						input_state.accumulate(action_id, KEY_ACTION_STRENGTH);
					}
				}
				_ => (),
			}
		}
	}

	pub fn on_device_event(&mut self, device_event: &DeviceEvent, mut notify_action: impl FnMut(ActionEvent)) {
		match device_event {
			DeviceEvent::MouseMotion { delta: (dx, dy) } => self.on_mouse_motion(*dx, -(*dy), &mut notify_action),
			DeviceEvent::MouseWheel { delta } => (),
			DeviceEvent::Motion { axis, value } => (),
			DeviceEvent::Button { button, state } => (),
			DeviceEvent::Key(kinput) => self.on_keyboard_input(kinput, &mut notify_action),
			_ => (),
		}
	}

	fn on_mouse_motion(&mut self, dx: f64, dy: f64, notify_action: &mut impl FnMut(ActionEvent)) {
		if !self.window_focused {
			return;
		}

		if relative_ne!(dx, 0.0) {
			let motion_type = if dx > 0.0 {
				MouseMotionType::PositiveX
			} else {
				MouseMotionType::NegativeX
			};

			self.binding_map.mouse_bindings.process_mouse_motion(
				motion_type,
				dx.abs(),
                                notify_action
			);
		}

		if relative_ne!(dy, 0.0) {
			let motion_type = if dy > 0.0 {
				MouseMotionType::PositiveY
			} else {
				MouseMotionType::NegativeY
			};

			self.binding_map.mouse_bindings.process_mouse_motion(
				motion_type,
				dy.abs(),
                                notify_action
			);
		}
	}

	fn on_keyboard_input(&mut self, kinput: &KeyboardInput, notify_action: &mut impl FnMut(ActionEvent)) {
		let key_code = match kinput.virtual_keycode {
			Some(key_code) => key_code,
			None => return,
		};

		if self.keyboard_state.is_key_repeat(key_code, kinput.state) {
			return;
		}
		self.keyboard_state.set_key_state(key_code, kinput.state);

		let KeyBinding {
			action_id,
			key_binding_type,
		} = match self.binding_map.get_key_binding(key_code) {
			Some(key_binding) => key_binding,
			None => return,
		};

		match key_binding_type {
			KeyBindingType::Simple(activator_state) => {
				if !self.window_focused {
					return;
				}

				if *activator_state == kinput.state {
					let action_event = ActionEvent {
						action_id: action_id.clone(),
						strength: KEY_ACTION_STRENGTH,
					};

					notify_action(action_event);
				}
			}
			KeyBindingType::Extended => {
				// If not focused, we only want to release keys that were pressed while focused.
				if !self.window_focused && (kinput.state != KeyState::Released) {
					return;
				}

				let mut input_state = self.input_state.lock().unwrap();

				match kinput.state {
					KeyState::Pressed => input_state.accumulate(action_id, KEY_ACTION_STRENGTH),
					KeyState::Released => input_state.reset(action_id),
				}
			}
		};
	}
}

struct KeyboardState {
	key_states: [KeyState; Self::MAX_KEY_CODE],
}

impl KeyboardState {
	const MAX_KEY_CODE: usize = KeyCode::Cut as usize;

	fn new() -> Self {
		Self {
			key_states: [KeyState::Released; Self::MAX_KEY_CODE],
		}
	}

	fn is_key_repeat(&self, key_code: KeyCode, key_state: KeyState) -> bool {
		self.get(key_code) == key_state
	}

	fn set_key_state(&mut self, key_code: KeyCode, key_state: KeyState) {
		*self.get_mut(key_code) = key_state;
	}

	fn get(&self, key_code: KeyCode) -> KeyState {
		self.key_states[key_code as usize]
	}

	fn get_mut(&mut self, key_code: KeyCode) -> &mut KeyState {
		&mut self.key_states[key_code as usize]
	}

	fn iter_presed(
		&self,
	) -> std::iter::FilterMap<
		std::iter::Enumerate<std::slice::Iter<'_, KeyState>>,
		for<'r> fn((usize, &'r KeyState)) -> Option<KeyCode>,
	> {
		fn f((kc, ks): (usize, &KeyState)) -> Option<KeyCode> {
			if *ks == KeyState::Pressed {
				Some(unsafe { std::mem::transmute::<u32, KeyCode>(kc as u32) })
			} else {
				None
			}
		}

		self.key_states
			.iter()
			.enumerate()
			.filter_map(f as fn((usize, &KeyState)) -> Option<KeyCode>)
	}
}

impl<'a> IntoIterator for &'a KeyboardState {
	type Item = (KeyCode, KeyState);

	type IntoIter = std::iter::Map<
		std::iter::Enumerate<std::slice::Iter<'a, KeyState>>,
		for<'r> fn((usize, &KeyState)) -> (KeyCode, KeyState),
	>;

	fn into_iter(self) -> Self::IntoIter {
		fn f((kc, ks): (usize, &KeyState)) -> (KeyCode, KeyState) {
			(unsafe { std::mem::transmute::<u32, KeyCode>(kc as u32) }, *ks)
		}

		self.key_states
			.iter()
			.enumerate()
			.map(f as fn((usize, &KeyState)) -> (KeyCode, KeyState))
	}
}

pub struct InputState {
	values: HashMap<ActionId, ActionStrength>,
}

impl InputState {
	pub fn poll(&mut self) -> HashMap<ActionId, ActionStrength> {
		/* self.values
		.iter()
		.filter_map(|(action, accum)| {
			if !accum.is_empty() {
				let avg = accum.iter().sum::<f32>() / accum.len() as f32;

				Some((action.clone(), avg))
			} else {
				None
			}
		})
		.collect() */
		self.values.clone()
	}

	fn accumulate(&mut self, action_id: &ActionId, strength: ActionStrength) {
		match self.values.get_mut(action_id) {
			Some(prev_strength) => {
				prev_strength.0 = f32::max(prev_strength.0, strength.0);
			}
			None => {
				self.values.insert(action_id.clone(), strength);
			}
		}
	}

	fn reset(&mut self, action_id: &ActionId) {
		self.values.remove(action_id);
	}
}

pub struct InputBindingMap {
	action_types: HashMap<ActionId, ActionType>,

	key_bindings: KeyBindingMap,
	mouse_bindings: MouseBindingMap,
}

impl InputBindingMap {
	pub fn new() -> Self {
		Self {
			action_types: HashMap::new(),

			key_bindings: KeyBindingMap::new(),
			mouse_bindings: MouseBindingMap::new(),
		}
	}

	pub fn bind_key(&mut self, action_id: &str, key_code: KeyCode, key_binding_type: KeyBindingType) {
		let action_type = ActionType::from(key_binding_type);

		if let Some(&prev_action_type) = self.action_types.get(action_id) {
			assert!(action_type == prev_action_type);
		} else {
			self.action_types.insert(String::from(action_id), action_type);
		}

		self.key_bindings.bind(key_code, action_id, key_binding_type);
	}

	pub fn bind_mouse_motion(&mut self, action_id: &str, motion_type: MouseMotionType, threshold: Option<f64>) {
		self.mouse_bindings.bind_motion(action_id, motion_type, threshold);
        }

	fn get_key_binding(&self, key_code: KeyCode) -> Option<&KeyBinding> {
		self.key_bindings.get(key_code)
	}
}

struct KeyBindingMap {
	key_bindings: HashMap<KeyCode, KeyBinding>,
}

impl KeyBindingMap {
	fn new() -> Self {
		Self {
			key_bindings: HashMap::new(),
		}
	}

	fn bind(&mut self, key_code: KeyCode, action_id: &str, key_binding_type: KeyBindingType) {
		self.key_bindings.insert(
			key_code,
			KeyBinding {
				action_id: String::from(action_id),
				key_binding_type,
			},
		);
	}

	fn get(&self, key_code: KeyCode) -> Option<&KeyBinding> {
		self.key_bindings.get(&key_code)
	}
}

struct MouseBindingMap {
	motion_bindings: EnumMap<MouseMotionType, Option<MouseMotionBinding>>,
}

impl MouseBindingMap {
	fn new() -> Self {
		Self {
			motion_bindings: enum_map! { _ => None },
		}
	}

	fn bind_motion(&mut self, action_id: &str, motion_type: MouseMotionType, threshold: Option<f64>) {
		assert!(self.motion_bindings[motion_type].is_none());

		self.motion_bindings[motion_type] = Some(MouseMotionBinding {
			action_id: ActionId::from(action_id),
			threshold: threshold.map(|t| (t, 0.0)),
		});
	}

	fn process_mouse_motion(&mut self, motion_type: MouseMotionType, delta: f64, notify_action: &mut impl FnMut(ActionEvent)) {
		let binding = match &mut self.motion_bindings[motion_type] {
			Some(binding) => binding,
			None => return,
		};

		match &mut binding.threshold {
			Some((threshold, accumulated)) => {
				*accumulated += delta;

				let quotient = (*accumulated / *threshold).trunc();
				*accumulated -= quotient;

				let quotient = quotient as usize;

                                for _ in 0..quotient {
                                        notify_action(ActionEvent {
                                                action_id: binding.action_id.clone(),
                                                strength: ActionStrength(delta as f32),
                                        });
                                }
			}
			None => notify_action(ActionEvent {
				action_id: binding.action_id.clone(),
				strength: ActionStrength(delta as f32),
			}),
		}
	}
}

struct MouseMotionBinding {
	action_id: ActionId,
	threshold: Option<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, Enum)]
pub enum MouseMotionType {
	PositiveX,
	NegativeX,
	PositiveY,
	NegativeY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionType {
	Simple,
	Extended,
}

impl From<KeyBindingType> for ActionType {
	fn from(key_binding_type: KeyBindingType) -> Self {
		match key_binding_type {
			KeyBindingType::Simple(_) => ActionType::Simple,
			KeyBindingType::Extended => ActionType::Extended,
		}
	}
}

#[derive(Debug, Clone)]
pub struct KeyBinding {
	pub action_id: ActionId,
	pub key_binding_type: KeyBindingType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyBindingType {
	Simple(KeyState),
	Extended,
}
