mod cartoon_pass;
mod game;
mod visuals;
mod water_visuals;
use pill_engine::game::create_game;

create_game!(crate::game::Game {}, pill_engine::game::PillGame);
