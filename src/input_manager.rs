use std::sync::{Arc, Mutex};

use crate::hashmap::{GetOrInsert, HashMap};
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

type ActionEventListener = Box<dyn FnMut(&ActionEvent)>;

pub struct InputManager {
	binding_map: InputBindingMap,

	action_event_listeners: Vec<ActionEventListener>,
	action_pollable_state: Arc<Mutex<ActionPollableState>>,

	mouse_input_processor: MouseInputProcessor,
	keyboard_input_processor: KeyboardInputProcessor,

	window_focused: bool,
}

impl InputManager {
	pub fn new() -> Self {
		Self {
			binding_map: InputBindingMap::new(),

			action_event_listeners: Vec::new(),
			action_pollable_state: Arc::new(Mutex::new(ActionPollableState { values: HashMap::new() })),

			mouse_input_processor: MouseInputProcessor::new(),
			keyboard_input_processor: KeyboardInputProcessor::new(),

			window_focused: true,
		}
	}

	pub fn register_listener(&mut self, listener: Box<dyn FnMut(&ActionEvent)>) {
		self.action_event_listeners.push(listener);
	}

	pub fn clone_continuous_actions_state(&mut self) -> Arc<Mutex<ActionPollableState>> {
		Arc::clone(&self.action_pollable_state)
	}

	pub fn push_input_binding_map(&mut self, map: InputBindingMap) {
		self.binding_map = map;
	}

	pub fn on_window_focused(&mut self, focused: bool) {
		self.window_focused = focused;

		self.keyboard_input_processor.on_window_focused(
			&self.binding_map.key_bindings,
			&self.action_pollable_state,
			focused,
		);
	}

	pub fn on_device_event(&mut self, device_event: &DeviceEvent) {
		let listeners = &mut self.action_event_listeners;

		match device_event {
			DeviceEvent::MouseMotion { delta: (dx, dy) } => {
				self.mouse_input_processor.process_mouse_motion(
					self.window_focused,
					&self.binding_map.mouse_bindings,
					*dx as f32,
					-*dy as f32,
					&mut |action| {
                                                listeners.iter_mut().for_each(|listener| listener(action));
                                        },
				);
			}
			DeviceEvent::MouseWheel { delta } => (),
			DeviceEvent::Motion { axis, value } => (),
			DeviceEvent::Button { button, state } => (),
			DeviceEvent::Key(input) => {
				self.keyboard_input_processor.process_keyboard_input(
					self.window_focused,
					&self.binding_map.key_bindings,
					&self.action_pollable_state,
					input,
					&mut |action| {
                                                listeners.iter_mut().for_each(|listener| listener(action));
                                        },
				);
			}
			_ => (),
		}
	}
}

pub struct ActionListener {
        events: ,

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

	/* fn iter_presed(
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
	} */

	fn pressed(&self) -> impl Iterator<Item = KeyCode> + '_ {
		self.key_states.iter().enumerate().filter_map(|(kc, ks)| match ks {
			KeyState::Pressed => Some(unsafe { std::mem::transmute::<u32, KeyCode>(kc as u32) }),
			KeyState::Released => None,
		})
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

pub struct ActionPollableState {
	values: HashMap<ActionId, ActionStrength>,
}

impl ActionPollableState {
	pub fn poll(&mut self) -> HashMap<ActionId, ActionStrength> {
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

	key_bindings: KeyBindings,
	mouse_bindings: MouseBindings,
}

impl InputBindingMap {
	pub fn new() -> Self {
		Self {
			action_types: HashMap::new(),

			key_bindings: KeyBindings::new(),
			mouse_bindings: MouseBindings::new(),
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

	pub fn bind_mouse_motion(&mut self, action_id: &str, motion_type: MouseMotionType, threshold: Option<f32>) {
		self.mouse_bindings.bind_motion(action_id, motion_type, threshold);
	}
}

struct KeyBindings {
	key_bindings: HashMap<KeyCode, KeyBinding>,
}

impl KeyBindings {
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

struct KeyboardInputProcessor {
	keyboard_state: KeyboardState,
}

impl KeyboardInputProcessor {
	fn new() -> Self {
		Self {
			keyboard_state: KeyboardState::new(),
		}
	}

	fn process_keyboard_input(
		&mut self,
		window_focused: bool,
		key_bindings: &KeyBindings,
		input_state: &Mutex<ActionPollableState>,
		input: &KeyboardInput,
		for_each_action: &mut impl FnMut(&ActionEvent),
	) {
		let key_code = match input.virtual_keycode {
			Some(kc) => kc,
			None => return,
		};

		if self.keyboard_state.is_key_repeat(key_code, input.state) {
			return;
		}

		self.keyboard_state.set_key_state(key_code, input.state);

		if !window_focused {
			return;
		}

		let binding = match key_bindings.get(key_code) {
			Some(b) => b,
			None => return,
		};

		match binding.key_binding_type {
			KeyBindingType::Simple(activator_state) => {
				if activator_state == input.state {
                                        let event = ActionEvent {
                                                action_id: binding.action_id.clone(),
						strength: KEY_ACTION_STRENGTH,
                                        };
					for_each_action(&event)
				}
			}
			KeyBindingType::Continuous => {
				let mut input_state = input_state.lock().unwrap();

				match input.state {
					KeyState::Pressed => {
						input_state.accumulate(&binding.action_id, KEY_ACTION_STRENGTH)
					}
					KeyState::Released => input_state.reset(&binding.action_id),
				}
			}
		}
	}

	fn on_window_focused(&self, key_bindings: &KeyBindings, input_state: &Mutex<ActionPollableState>, window_focused: bool) {
		for key_code in self.keyboard_state.pressed() {
			let binding = match key_bindings.get(key_code) {
				Some(b) => b,
				None => continue,
			};

			match binding.key_binding_type {
				KeyBindingType::Continuous => {
					let mut input_state = input_state.lock().unwrap();

					if !window_focused {
						input_state.reset(&binding.action_id);
					} else {
						input_state.accumulate(&binding.action_id, KEY_ACTION_STRENGTH);
					}
				}
				_ => (),
			}
		}
	}
}

struct MouseBindings {
	motion_bindings: EnumMap<MouseMotionType, Option<MouseMotionBinding>>,
}

impl MouseBindings {
	fn new() -> Self {
		Self {
			motion_bindings: enum_map! { _ => None },
		}
	}

	fn bind_motion(&mut self, action_id: &str, motion_type: MouseMotionType, threshold: Option<f32>) {
		self.motion_bindings[motion_type] = Some(MouseMotionBinding {
			action_id: ActionId::from(action_id),
			threshold,
		});
	}

	fn get_motion_binding(&self, motion_type: MouseMotionType) -> Option<&MouseMotionBinding> {
		self.motion_bindings[motion_type].as_ref()
	}
}

struct MouseMotionBinding {
	action_id: ActionId,
	threshold: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub enum MouseMotionAxis {
	X,
	Y,
}

#[derive(Debug, Clone, Copy, Enum)]
pub enum MouseMotionType {
	PositiveX,
	NegativeX,
	PositiveY,
	NegativeY,
}

struct MouseInputProcessor {
	accumulators: HashMap<ActionId, f32>,
}

impl MouseInputProcessor {
	fn new() -> Self {
		Self {
			accumulators: HashMap::new(),
		}
	}

	fn process_mouse_motion(
		&mut self,
		window_focused: bool,
		mouse_bindings: &MouseBindings,
		dx: f32,
		dy: f32,
		for_each_action: &mut impl FnMut(&ActionEvent),
	) {
		if !window_focused {
			return;
		}

		if let Some(motion_type) = Self::figure_motion_type(MouseMotionAxis::X, dx) {
			self.process_directional_motion(mouse_bindings, motion_type, dx.abs(), for_each_action)
		}

		if let Some(motion_type) = Self::figure_motion_type(MouseMotionAxis::Y, dy) {
			self.process_directional_motion(mouse_bindings, motion_type, dy.abs(), for_each_action)
		}
	}

	fn figure_motion_type(motion_axis: MouseMotionAxis, delta: f32) -> Option<MouseMotionType> {
		if relative_eq!(delta, 0.0) {
			return None;
		}

		let motion_type = if delta > 0.0 {
			match motion_axis {
				MouseMotionAxis::X => MouseMotionType::PositiveX,
				MouseMotionAxis::Y => MouseMotionType::PositiveY,
			}
		} else {
			match motion_axis {
				MouseMotionAxis::X => MouseMotionType::NegativeX,
				MouseMotionAxis::Y => MouseMotionType::NegativeY,
			}
		};

		Some(motion_type)
	}

	fn process_directional_motion(
		&mut self,
		mouse_bindings: &MouseBindings,
		motion_type: MouseMotionType,
		delta: f32,
		for_each_action: &mut impl FnMut(&ActionEvent),
	) {
		let binding = match mouse_bindings.get_motion_binding(motion_type) {
			Some(binding) => binding,
			None => return,
		};

		let accumulator = self.accumulators.get_mut_or_insert(&binding.action_id, 0.0);

		match binding.threshold {
			Some(threshold) => {
				*accumulator += delta;

				let quotient = (*accumulator / threshold).trunc();
				*accumulator -= quotient;

                                let event = ActionEvent {
					action_id: binding.action_id.clone(),
					strength: ActionStrength(delta as f32),
				};
				(0..quotient as usize).for_each(|_| for_each_action(&event))
			}
			None => {
				let event = ActionEvent {
					action_id: binding.action_id.clone(),
					strength: ActionStrength(delta as f32),
				};
				for_each_action(&event)
			}
		}
	}
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
			KeyBindingType::Continuous => ActionType::Extended,
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
	Continuous,
}
