/* pub const EXIT: &str = "exit";
pub const TOGGLE_CURSOR: &str = "toggle-cursor";
pub const CYCLE_WINDOW_MODE: &str = "cycle-window-mode";
pub const MOVE_FORWARD: &str = "move-forward";
pub const MOVE_BACKWARD: &str = "move-backward";
pub const MOVE_RIGHTWARD: &str = "move-rightward";
pub const MOVE_LEFTWARD: &str = "move-leftward";
pub const YAW_POSITIVE: &str = "yaw-positive";
pub const YAW_NEGATIVE: &str = "yaw-negative";
pub const PITCH_POSITIVE: &str = "pitch-positive";
pub const PITCH_NEGATIVE: &str = "pitch-negative"; */

pub struct Action {
	id: ActionId,
	kind: ActionKind,
}

impl Action {
	pub const fn new(id: &'static str, kind: ActionKind) -> Self {
		Self { id: ActionId(id), kind }
	}
}

pub struct ActionId(&'static str);

pub enum ActionKind {
	Discrete,
	Continuous,
}

pub const JUMP: Action = Action::new("jump", ActionKind::Continuous);
pub const EXIT: Action = Action::new("exit", ActionKind::Discrete);
pub const TOGGLE_CURSOR: Action = Action::new("toggle-cursor", ActionKind::Discrete);
pub const CYCLE_WINDOW_MODE: Action = Action::new("cycle-window-mode", ActionKind::Discrete);
pub const MOVE_FORWARD: Action = Action::new("move-forward", ActionKind::Continuous);
pub const MOVE_BACKWARD: Action = Action::new("move-backward", ActionKind::Continuous);
pub const MOVE_RIGHTWARD: Action = Action::new("move-rightward", ActionKind::Continuous);
pub const MOVE_LEFTWARD: Action = Action::new("move-leftward", ActionKind::Continuous);
pub const YAW_POSITIVE: Action = Action::new("yaw-positive", ActionKind::Continuous);
pub const YAW_NEGATIVE: Action = Action::new("yaw-negative", ActionKind::Continuous);
pub const PITCH_POSITIVE: Action = Action::new("pitch-positive", ActionKind::Continuous);
pub const PITCH_NEGATIVE: Action = Action::new("pitch-negative", ActionKind::Continuous);
