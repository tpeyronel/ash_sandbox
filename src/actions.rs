use std::ops::Deref;

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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Action {
	pub id: ActionId,
	pub kind: ActionKind,
}

impl Action {
	pub const fn new(id: &'static str, kind: ActionKind) -> Self {
		Self { id: ActionId(id), kind }
	}
}

impl Deref for Action {
	type Target = &'static str;

	fn deref(&self) -> &Self::Target {
		&self.id.0
	}
}

impl PartialEq<ActionId> for Action {
	fn eq(&self, other: &ActionId) -> bool {
		self.id.0 == other.0
	}
}

impl PartialEq<Action> for ActionId {
	fn eq(&self, other: &Action) -> bool {
		self.0 == other.id.0
	}
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct ActionId(pub &'static str);

impl From<&'static str> for ActionId {
	fn from(s: &'static str) -> Self {
		Self(s)
	}
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ActionKind {
	Discrete,
	Continuous,
}

// pub const JUMP: &Action = &Action::new("jump", ActionKind::Continuous);
// pub const EXIT: &Action = &Action::new("exit", ActionKind::Discrete);
// pub const TOGGLE_CURSOR: &Action = &Action::new("toggle-cursor", ActionKind::Discrete);
// pub const CYCLE_WINDOW_MODE: &Action = &Action::new("cycle-window-mode", ActionKind::Discrete);
// pub const MOVE_FORWARD: &Action = &Action::new("move-forward", ActionKind::Continuous);
// pub const MOVE_BACKWARD: &Action = &Action::new("move-backward", ActionKind::Continuous);
// pub const MOVE_RIGHTWARD: &Action = &Action::new("move-rightward", ActionKind::Continuous);
// pub const MOVE_LEFTWARD: &Action = &Action::new("move-leftward", ActionKind::Continuous);
// pub const YAW_POSITIVE: &Action = &Action::new("yaw-positive", ActionKind::Continuous);
// pub const YAW_NEGATIVE: &Action = &Action::new("yaw-negative", ActionKind::Continuous);
// pub const PITCH_POSITIVE: &Action = &Action::new("pitch-positive", ActionKind::Continuous);
// pub const PITCH_NEGATIVE: &Action = &Action::new("pitch-negative", ActionKind::Continuous);

pub const JUMP: ActionId = ActionId("jump");
pub const EXIT: ActionId = ActionId("exit");
pub const TOGGLE_CURSOR: ActionId = ActionId("toggle-cursor");
pub const CYCLE_WINDOW_MODE: ActionId = ActionId("cycle-window-mode");
pub const MOVE_FORWARD: ActionId = ActionId("move-forward");
pub const MOVE_BACKWARD: ActionId = ActionId("move-backward");
pub const MOVE_RIGHTWARD: ActionId = ActionId("move-rightward");
pub const MOVE_LEFTWARD: ActionId = ActionId("move-leftward");
pub const MOVE_UPWARD: ActionId = ActionId("move-upward");
pub const MOVE_DOWNARD: ActionId = ActionId("move-downward");
pub const YAW_POSITIVE: ActionId = ActionId("yaw-positive");
pub const YAW_NEGATIVE: ActionId = ActionId("yaw-negative");
pub const PITCH_POSITIVE: ActionId = ActionId("pitch-positive");
pub const PITCH_NEGATIVE: ActionId = ActionId("pitch-negative");
pub const ROLL_POSITIVE: ActionId = ActionId("roll-positive");
pub const ROLL_NEGATIVE: ActionId = ActionId("roll-negative");
