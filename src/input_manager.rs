use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self},
        Arc, Mutex,
};

use crate::{
        actions::ActionId,
        constants::PIXELS_PER_UNIT,
        hashmap::{GetOrInsert, HashMap},
};
use enum_map::EnumMap;
use log::error;
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

#[derive(Debug, Clone, Copy, Default)]
pub struct ActionStrength(pub f32);

const KEY_ACTION_STRENGTH: ActionStrength = ActionStrength(1.0);

pub struct InputManager {
        binding_map: InputBindingMap,

        action_event_senders: Vec<mpsc::Sender<ActionEvent>>,
        pollable_actions: Arc<Mutex<HashMap<ActionId, ActionStrength>>>,

        mouse_input_processor: MouseInputProcessor,
        keyboard_input_processor: KeyboardInputProcessor,

        should_dispatch_actions: bool,
        should_poll_actions: Arc<AtomicBool>,
}

impl InputManager {
        pub fn new(should_dispatch_actions: bool) -> Self {
                Self {
                        binding_map: InputBindingMap::new(),

                        action_event_senders: Vec::new(),
                        pollable_actions: Arc::new(Mutex::new(HashMap::new())),

                        mouse_input_processor: MouseInputProcessor::new(),
                        keyboard_input_processor: KeyboardInputProcessor::new(),

                        should_dispatch_actions,
                        should_poll_actions: Arc::new(AtomicBool::new(should_dispatch_actions)),
                }
        }

        pub fn set_dispatch_actions(&mut self, dispatch_actions: bool) {
                self.should_dispatch_actions = dispatch_actions;
                self.should_poll_actions.store(dispatch_actions, Ordering::Relaxed);
        }

        pub fn create_action_receiver(&mut self) -> ActionReceiver {
                let (action_events_tx, action_events_rx) = mpsc::channel();

                self.action_event_senders.push(action_events_tx);

                ActionReceiver::new(
                        Arc::clone(&self.should_poll_actions),
                        self.pollable_actions.clone(),
                        action_events_rx,
                )
        }

        pub fn push_input_binding_map(&mut self, map: InputBindingMap) {
                self.binding_map = map;
        }

        pub fn on_device_event(&mut self, device_event: &DeviceEvent) {
                let action_events_senders = &self.action_event_senders;
                let should_dispatch_actions = self.should_dispatch_actions;

                let mut dispatch_action = |action_event: ActionEvent| {
                        if !should_dispatch_actions {
                                return;
                        }

                        for tx in action_events_senders {
                                if let Err(err) = tx.send(action_event.clone()) {
                                        error!("Error occurred sending action event: {}", err);
                                }
                        }
                };

                match device_event {
                        DeviceEvent::MouseMotion { delta: (dx, dy) } => {
                                self.mouse_input_processor.process_mouse_motion(
                                        &self.binding_map.mouse_bindings,
                                        *dx as f32,
                                        -*dy as f32,
                                        &mut dispatch_action,
                                );
                        },
                        DeviceEvent::MouseWheel { .. } => (),
                        DeviceEvent::Motion { .. } => (),
                        DeviceEvent::Button { .. } => (),
                        DeviceEvent::Key(input) => {
                                self.keyboard_input_processor.process_keyboard_input(
                                        &self.binding_map.key_bindings,
                                        &self.pollable_actions,
                                        input,
                                        &mut dispatch_action,
                                );
                        },
                        _ => (),
                }
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

        fn set_key_state(&mut self, key_code: KeyCode, key_state: KeyState) {
                *self.get_mut(key_code) = key_state;
        }

        fn get(&self, key_code: KeyCode) -> KeyState {
                self.key_states[key_code as usize]
        }

        fn get_mut(&mut self, key_code: KeyCode) -> &mut KeyState {
                &mut self.key_states[key_code as usize]
        }

        #[allow(unused)]
        fn pressed_keys(&self) -> impl Iterator<Item = KeyCode> + '_ {
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

pub struct ActionReceiver {
        should_poll_actions: Arc<AtomicBool>,
        pollable_actions: Arc<Mutex<HashMap<ActionId, ActionStrength>>>,
        action_events: mpsc::Receiver<ActionEvent>,
}

impl ActionReceiver {
        pub fn new(
                should_poll_actions: Arc<AtomicBool>,
                pollable_actions: Arc<Mutex<HashMap<ActionId, ActionStrength>>>,
                action_events: mpsc::Receiver<ActionEvent>,
        ) -> Self {
                Self {
                        should_poll_actions,
                        pollable_actions,
                        action_events,
                }
        }

        pub fn receive(&self, delta_time: f32) -> Vec<(ActionId, ActionStrength)> {
                let action_events = self.action_events.try_iter().map(|e| (e.action_id, e.strength));

                self.poll_actions()
                        .into_iter()
                        .map(|(id, s)| (id, ActionStrength(s.0 * delta_time)))
                        .chain(action_events)
                        .collect()
        }

        fn poll_actions(&self) -> HashMap<ActionId, ActionStrength> {
                if self.should_poll_actions.load(Ordering::Relaxed) {
                        self.pollable_actions
                                .lock()
                                .expect("Failed to lock pollable actions!")
                                .clone()
                } else {
                        HashMap::new()
                }
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

        pub fn bind_key(&mut self, action_id: ActionId, key_code: KeyCode, key_binding_type: KeyBindingType) {
                let action_type = ActionType::from(key_binding_type);

                if let Some(&prev_action_type) = self.action_types.get(&action_id) {
                        assert_eq!(action_type, prev_action_type);
                } else {
                        self.action_types.insert(action_id.clone(), action_type);
                }

                self.key_bindings.bind(key_code, action_id, key_binding_type);
        }

        pub fn bind_mouse_motion(&mut self, action_id: ActionId, motion_type: MouseMotionType, threshold: Option<f32>) {
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

        fn bind(&mut self, key_code: KeyCode, action_id: ActionId, key_binding_type: KeyBindingType) {
                self.key_bindings.insert(
                        key_code,
                        KeyBinding {
                                action_id,
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
                key_bindings: &KeyBindings,
                pollable_actions: &Mutex<HashMap<ActionId, ActionStrength>>,
                input: &KeyboardInput,
                dispatch_action: &mut impl FnMut(ActionEvent),
        ) {
                let key_code = match input.virtual_keycode {
                        Some(kc) => kc,
                        None => return,
                };

                if self.keyboard_state.get(key_code) == input.state {
                        return;
                }
                self.keyboard_state.set_key_state(key_code, input.state);

                let binding = match key_bindings.get(key_code) {
                        Some(b) => b,
                        None => return,
                };

                match binding.key_binding_type {
                        KeyBindingType::Simple(activator_state) => {
                                if activator_state == input.state {
                                        dispatch_action(ActionEvent {
                                                action_id: binding.action_id.clone(),
                                                strength: KEY_ACTION_STRENGTH,
                                        });
                                }
                        },
                        KeyBindingType::Continuous => {
                                let mut pollable_actions = pollable_actions
                                        .lock()
                                        .expect("Error ocurred locking pollable actions!");

                                match input.state {
                                        KeyState::Pressed => {
                                                *pollable_actions.get_mut_or_insert(
                                                        &binding.action_id,
                                                        ActionStrength::default(),
                                                ) = KEY_ACTION_STRENGTH;
                                        },
                                        KeyState::Released => {
                                                pollable_actions.remove(&binding.action_id);
                                        },
                                }
                        },
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

        fn bind_motion(&mut self, action_id: ActionId, motion_type: MouseMotionType, threshold: Option<f32>) {
                self.motion_bindings[motion_type] = Some(MouseMotionBinding { action_id, threshold });
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
                mouse_bindings: &MouseBindings,
                dx: f32,
                dy: f32,
                dispatch_action: &mut impl FnMut(ActionEvent),
        ) {
                if let Some(motion_type) = Self::resolve_motion_type(MouseMotionAxis::X, dx) {
                        self.process_directional_motion(mouse_bindings, motion_type, dx.abs(), dispatch_action)
                }

                if let Some(motion_type) = Self::resolve_motion_type(MouseMotionAxis::Y, dy) {
                        self.process_directional_motion(mouse_bindings, motion_type, dy.abs(), dispatch_action)
                }
        }

        fn resolve_motion_type(motion_axis: MouseMotionAxis, delta: f32) -> Option<MouseMotionType> {
                if delta == 0.0 {
                        return None;
                }

                Some(match (motion_axis, delta > 0.0) {
                        (MouseMotionAxis::X, true) => MouseMotionType::PositiveX,
                        (MouseMotionAxis::X, false) => MouseMotionType::NegativeX,
                        (MouseMotionAxis::Y, true) => MouseMotionType::PositiveY,
                        (MouseMotionAxis::Y, false) => MouseMotionType::NegativeY,
                })
        }

        fn process_directional_motion(
                &mut self,
                mouse_bindings: &MouseBindings,
                motion_type: MouseMotionType,
                delta: f32,
                dispatch_action: &mut impl FnMut(ActionEvent),
        ) {
                let binding = match mouse_bindings.get_motion_binding(motion_type) {
                        Some(binding) => binding,
                        None => return,
                };

                let accumulator = self.accumulators.get_mut_or_insert(&binding.action_id, 0.0);
                let strength = ActionStrength(delta as f32 * (1.0 / PIXELS_PER_UNIT));

                match binding.threshold {
                        Some(threshold) => {
                                *accumulator += delta;

                                let quotient = (*accumulator / threshold).trunc();
                                *accumulator -= quotient;

                                for _ in 0..quotient as u32 {
                                        dispatch_action(ActionEvent {
                                                action_id: binding.action_id.clone(),
                                                strength,
                                        })
                                }
                        },
                        None => dispatch_action(ActionEvent {
                                action_id: binding.action_id.clone(),
                                strength,
                        }),
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
