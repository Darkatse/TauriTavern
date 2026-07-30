mod roll;
mod specs;

pub(super) use roll::roll;
pub(super) use specs::dice_roll_spec;

pub(super) const DICE_ROLL: &str = "dice.roll";

const MAX_DICE: usize = 100;
const MAX_SIDES: u64 = 1_000_000;
const MAX_ABS_MODIFIER: i64 = 1_000_000;
