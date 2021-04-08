use hashbrown::HashMap;
#[allow(unused_imports)]
use log::info;
use std::collections::VecDeque;
use winit::event::{ElementState, KeyboardInput, ModifiersState, VirtualKeyCode};

const KEY_COUNT: usize = VirtualKeyCode::Cut as usize;

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum InputMessage {
        Exit,
        ToggleCursor,
        ToggleFullscreen,
        PlayerMoveForward,
        PlayerMoveBackward,
        PlayerMoveRight,
        PlayerMoveLeft,
        PlayerMoveUp,
        PlayerMoveDown,
}

pub struct InputManager {
        old_key_states: [ElementState; KEY_COUNT],
        key_states: [ElementState; KEY_COUNT],
        old_modifiers_state: ModifiersState,
        modifiers_state: ModifiersState,
        // old_key_states: HashMap<ScanCode, ElementState>,
        // key_states: HashMap<ScanCode, ElementState>,
        //kpress_listeners: HashMap<VirtualKeyCode, Vec<Box<dyn FnMut()>>>,
        //krelease_listeners: HashMap<VirtualKeyCode, Vec<Box<dyn FnMut()>>>,
        key_press_messages: HashMap<VirtualKeyCode, Vec<InputMessage>>,
        key_release_messages: HashMap<VirtualKeyCode, Vec<InputMessage>>,
        message_queue: VecDeque<InputMessage>,
}

impl InputManager {
        pub fn new() -> Self {
                Self {
                        old_key_states: [ElementState::Released; KEY_COUNT],
                        key_states: [ElementState::Released; KEY_COUNT],
                        old_modifiers_state: ModifiersState::empty(),
                        modifiers_state: ModifiersState::empty(),
                        //kpress_listeners: HashMap::new(),
                        //krelease_listeners: HashMap::new(),
                        key_press_messages: HashMap::new(),
                        key_release_messages: HashMap::new(),
                        message_queue: VecDeque::new(),
                }
        }

        pub fn update(&mut self) {
                self.old_key_states = self.key_states.clone();
                self.old_modifiers_state = self.modifiers_state;
        }

        pub fn next_message(&mut self) -> Option<InputMessage> {
                self.message_queue.pop_front()
        }

        pub fn on_keyboard_input(&mut self, kinput: &KeyboardInput) {
                let virtual_kcode = match kinput.virtual_keycode {
                        Some(v) => v,
                        None => return,
                };

                self.key_states[virtual_kcode as usize] = kinput.state;

                let prev_state = self.old_key_states[virtual_kcode as usize];
                let curr_state = self.key_states[virtual_kcode as usize];

                match (prev_state, curr_state) {
                        (ElementState::Pressed, ElementState::Pressed) => {}
                        (ElementState::Pressed, ElementState::Released) => {
                                if let Some(messages) = self.key_release_messages.get(&virtual_kcode) {
                                        for &message in messages {
                                                self.message_queue.push_back(message);
                                        }
                                }
                                /* if let Some(listeners) = self.krelease_listeners.get_mut(&virtual_kcode) {
                                        for listener in listeners {
                                                (*listener)();
                                        }
                                } */
                        }
                        (ElementState::Released, ElementState::Pressed) => {}
                        (ElementState::Released, ElementState::Released) => {
                                if let Some(messages) = self.key_press_messages.get(&virtual_kcode) {
                                        for &message in messages {
                                                self.message_queue.push_back(message);
                                        }
                                }
                                /* if let Some(listeners) = self.kpress_listeners.get_mut(&virtual_kcode) {
                                        for listener in listeners {
                                                (*listener)();
                                        }
                                } */
                        }
                }
        }

        pub fn on_modifiers_changed(&mut self, modifiers_state: ModifiersState) {
                self.modifiers_state = modifiers_state;
        }

        #[allow(dead_code)]
        pub fn key_pressed(&self, virtual_kcode: VirtualKeyCode) -> bool {
                self.key_states[virtual_kcode as usize] == ElementState::Pressed
                //self.key_states.get(&scan_code).map_or(false, |s| *s == ElementState::Pressed)
        }

        #[allow(dead_code)]
        pub fn key_released(&self, virtual_kcode: VirtualKeyCode) -> bool {
                self.key_states[virtual_kcode as usize] == ElementState::Released
                //self.key_states.get(&scan_code).map_or(true, |s| *s == ElementState::Released)
        }

        #[allow(dead_code)]
        pub fn on_key_press(&mut self, virtual_kcode: VirtualKeyCode, message: InputMessage) {
                self.key_press_messages
                        .entry(virtual_kcode)
                        .or_insert(Vec::new())
                        .push(message);
        }

        #[allow(dead_code)]
        pub fn on_key_release(&mut self, virtual_kcode: VirtualKeyCode, message: InputMessage) {
                self.key_release_messages
                        .entry(virtual_kcode)
                        .or_insert(Vec::new())
                        .push(message);
        }

        #[allow(dead_code)]
        pub fn all_modifiers(&self, modifiers_state: ModifiersState) -> bool {
                self.modifiers_state.contains(modifiers_state)
        }

        #[allow(dead_code)]
        pub fn any_modifiers(&self, modifiers_state: ModifiersState) -> bool {
                (self.modifiers_state & modifiers_state) != ModifiersState::empty()
        }
}
