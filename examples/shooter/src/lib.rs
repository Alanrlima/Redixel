pub mod common;
pub mod effects;
pub mod game;

use game::Shooter;

redixel::entry_point!(Shooter::new());
