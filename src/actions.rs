#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct ActionId(pub &'static str);

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
