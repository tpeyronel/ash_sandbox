use std::ops::{Index, IndexMut};

use crate::{
        actions::ActionId,
        constants::PIXELS_PER_UNIT,
        hashmap::{GetOrInsert, HashMap},
};
use enum_map::EnumMap;
#[allow(unused_imports)]
use log::{error, info};
use winit::{
        event::{DeviceEvent, RawKeyEvent},
        keyboard::PhysicalKey,
};

pub type KeyCode = winit::keyboard::KeyCode;
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

        mouse_input_processor: MouseInputProcessor,
        keyboard_input_processor: KeyboardInputProcessor,

        action_events: Vec<ActionEvent>,
}

impl InputManager {
        pub fn new() -> Self {
                Self {
                        binding_map: InputBindingMap::new(),

                        action_events: Vec::new(),

                        mouse_input_processor: MouseInputProcessor::new(),
                        keyboard_input_processor: KeyboardInputProcessor::new(),
                }
        }

        pub fn push_input_binding_map(&mut self, map: InputBindingMap) {
                self.binding_map = map;
                self.keyboard_input_processor
                        .set_key_bindings(self.binding_map.key_bindings.clone());
                self.mouse_input_processor
                        .set_mouse_bindings(self.binding_map.mouse_bindings.clone());
        }

        pub fn drain_events(&mut self) -> Vec<ActionEvent> {
                std::mem::replace(&mut self.action_events, Vec::new())
        }

        pub fn poll_actions(&mut self) -> HashMap<ActionId, ActionStrength> {
                let mut pollable_actions = HashMap::new();

                if let Some(actions) = self.mouse_input_processor.poll_actions() {
                        pollable_actions.extend(actions);
                }

                if let Some(actions) = self.keyboard_input_processor.poll_actions() {
                        pollable_actions.extend(actions);
                }

                pollable_actions
        }

        pub fn set_ignore_mouse(&mut self, ignore_mouse: bool) {
                self.mouse_input_processor.enabled = !ignore_mouse;
        }

        pub fn set_ignore_keyboard(&mut self, ignore_keyboard: bool) {
                self.keyboard_input_processor.enabled = !ignore_keyboard;
        }

        pub fn on_device_event(&mut self, device_event: &DeviceEvent) {
                match device_event {
                        DeviceEvent::MouseMotion { delta: (dx, dy) } => {
                                self.mouse_input_processor.process_mouse_motion(
                                        *dx as f32,
                                        -*dy as f32,
                                        &mut self.action_events,
                                );
                        },
                        DeviceEvent::MouseWheel { .. } => (),
                        DeviceEvent::Motion { .. } => (),
                        DeviceEvent::Button { .. } => (),
                        DeviceEvent::Key(input) => {
                                self.keyboard_input_processor
                                        .process_keyboard_input(input, &mut self.action_events);
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

        fn get(&self, key_code: KeyCode) -> &KeyState {
                &self.key_states[key_code as usize]
        }

        fn get_mut(&mut self, key_code: KeyCode) -> &mut KeyState {
                &mut self.key_states[key_code as usize]
        }

        #[allow(unused)]
        fn key_states(&self) -> impl Iterator<Item = (KeyCode, KeyState)> + '_ {
                self.key_states
                        .iter()
                        .enumerate()
                        .map(|(kc, ks)| (unsafe { std::mem::transmute::<u8, KeyCode>(kc as u8) }, *ks))
        }

        #[allow(unused)]
        fn pressed_keys(&self) -> impl Iterator<Item = KeyCode> + '_ {
                self.key_states.iter().enumerate().filter_map(|(kc, ks)| match ks {
                        KeyState::Pressed => Some(unsafe { std::mem::transmute::<u8, KeyCode>(kc as u8) }),
                        KeyState::Released => None,
                })
        }
}

impl Index<KeyCode> for KeyboardState {
        type Output = KeyState;

        fn index(&self, index: KeyCode) -> &Self::Output {
                self.get(index)
        }
}

impl IndexMut<KeyCode> for KeyboardState {
        fn index_mut(&mut self, index: KeyCode) -> &mut Self::Output {
                self.get_mut(index)
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
                        (unsafe { std::mem::transmute::<u8, KeyCode>(kc as u8) }, *ks)
                }

                self.key_states
                        .iter()
                        .enumerate()
                        .map(f as fn((usize, &KeyState)) -> (KeyCode, KeyState))
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

#[derive(Clone)]
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
        key_bindings: KeyBindings,
        keyboard_state: KeyboardState,
        pollable_actions: HashMap<ActionId, ActionStrength>,
        enabled: bool,
}

impl KeyboardInputProcessor {
        fn new() -> Self {
                Self {
                        key_bindings: KeyBindings::new(),
                        keyboard_state: KeyboardState::new(),
                        pollable_actions: HashMap::new(),
                        enabled: true,
                }
        }

        fn poll_actions(&self) -> Option<HashMap<ActionId, ActionStrength>> {
                self.enabled.then(|| self.pollable_actions.clone())
        }

        fn set_key_bindings(&mut self, key_bindings: KeyBindings) {
                self.key_bindings = key_bindings;
                self.pollable_actions.clear();

                for (key_code, key_state) in self.keyboard_state.key_states() {
                        let binding = match self.key_bindings.get(key_code) {
                                Some(b) => b,
                                None => continue,
                        };

                        match binding.key_binding_type {
                                KeyBindingType::Simple(_) => (),
                                KeyBindingType::Continuous => match key_state {
                                        KeyState::Pressed => {
                                                self.pollable_actions
                                                        .insert(binding.action_id.clone(), KEY_ACTION_STRENGTH);
                                        },
                                        KeyState::Released => (),
                                },
                        }
                }
        }

        fn process_keyboard_input(&mut self, input: &RawKeyEvent, action_events: &mut Vec<ActionEvent>) {
                let key_code = match input.physical_key {
                        PhysicalKey::Code(kc) => kc,
                        PhysicalKey::Unidentified(_) => return,
                };

                // Key repeat, ignore
                if self.keyboard_state[key_code] == input.state {
                        return;
                }

                self.keyboard_state[key_code] = input.state;

                let binding = match self.key_bindings.get(key_code) {
                        Some(b) => b,
                        None => return,
                };

                match binding.key_binding_type {
                        KeyBindingType::Simple(activator_state) => {
                                if !self.enabled {
                                        return;
                                }

                                if activator_state == input.state {
                                        action_events.push(ActionEvent {
                                                action_id: binding.action_id.clone(),
                                                strength: KEY_ACTION_STRENGTH,
                                        })
                                }
                        },
                        KeyBindingType::Continuous => match input.state {
                                KeyState::Pressed => {
                                        self.pollable_actions
                                                .insert(binding.action_id.clone(), KEY_ACTION_STRENGTH);
                                },
                                KeyState::Released => {
                                        self.pollable_actions.remove(&binding.action_id);
                                },
                        },
                }
        }
}

#[derive(Clone)]
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

#[derive(Clone)]
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
        mouse_bindings: MouseBindings,
        accumulators: HashMap<ActionId, f32>,

        pollable_actions: HashMap<ActionId, ActionStrength>,
        enabled: bool,
}

impl MouseInputProcessor {
        fn new() -> Self {
                Self {
                        mouse_bindings: MouseBindings::new(),
                        accumulators: HashMap::new(),
                        pollable_actions: HashMap::new(),
                        enabled: true,
                }
        }

        fn poll_actions(&self) -> Option<HashMap<ActionId, ActionStrength>> {
                self.enabled.then(|| self.pollable_actions.clone())
        }

        fn set_mouse_bindings(&mut self, mouse_bindings: MouseBindings) {
                self.mouse_bindings = mouse_bindings;
        }

        fn process_mouse_motion(&mut self, dx: f32, dy: f32, action_events: &mut Vec<ActionEvent>) {
                if !self.enabled {
                        return;
                }

                if let Some(motion_type) = Self::determine_motion_type(MouseMotionAxis::X, dx) {
                        self.process_directional_motion(motion_type, dx.abs(), action_events);
                }

                if let Some(motion_type) = Self::determine_motion_type(MouseMotionAxis::Y, dy) {
                        self.process_directional_motion(motion_type, dy.abs(), action_events);
                }
        }

        fn determine_motion_type(motion_axis: MouseMotionAxis, delta: f32) -> Option<MouseMotionType> {
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
                motion_type: MouseMotionType,
                delta: f32,
                action_events: &mut Vec<ActionEvent>,
        ) {
                let binding = match self.mouse_bindings.get_motion_binding(motion_type) {
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
                                        action_events.push(ActionEvent {
                                                action_id: binding.action_id.clone(),
                                                strength,
                                        });
                                }
                        },
                        None => action_events.push(ActionEvent {
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
