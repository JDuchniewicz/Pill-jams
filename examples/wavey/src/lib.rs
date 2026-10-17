mod cartoon_pass;
mod game;
mod player;
mod uv_pass;
mod visuals;
mod water;
mod water_visuals;
use pill_engine::game::create_game;

create_game!(crate::game::Game {}, pill_engine::game::PillGame);
