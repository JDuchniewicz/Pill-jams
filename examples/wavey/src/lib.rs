mod background_pass;
mod cartoon_pass;
mod container;
mod duck;
mod effects;
mod game;
mod particle_system;
mod player;
mod post_process;
mod sounds;
mod visuals;
mod water;
mod water_floating;
mod water_mesh;
mod water_visuals;
use pill_engine::game::create_game;

create_game!(crate::game::Game {}, pill_engine::game::PillGame);
