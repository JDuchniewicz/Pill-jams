use pill_engine::game::*;

use crate::game::{add_bomb, PlayerComponent};

// TODO: for now we are playing locally -> maybe also play over network
const NUM_PLAYERS: u8 = 2;
const MOVEMENT_SPEED: f32 = 2.0;
const DZ: f32 = 0.1;

pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    engine.add_system("player_movement_system", player_movement_system)?;
    Ok(())
}

fn player_movement_system(engine: &mut Engine) -> Result<()> {
    // Gather input first: the immutable `InputComponent` borrow must end before `add_bomb`,
    // which borrows `GameState` mutably.
    let (movements, _wobbles, drop_bombs, delta_time) = {
        let input = engine.get_global_component::<InputComponent>()?;

        // movements
        let movements: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let l_stick_y = input.get_gamepad_axis(id, GamepadAxis::LeftStickY);
            let l_stick_x = input.get_gamepad_axis(id, GamepadAxis::LeftStickX);
            (l_stick_x, l_stick_y)
        });

        // wobbles
        let wobbles: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let r_stick_y = input.get_gamepad_axis(id, GamepadAxis::RightStickY);
            let r_stick_x = input.get_gamepad_axis(id, GamepadAxis::RightStickX);
            (r_stick_x, r_stick_y)
        });

        // actions: drop a bomb
        let drop_bombs: [bool; NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            println!(
                "Player {} pressed A: {}",
                i,
                input.get_gamepad_button_pressed(id, GamepadButton::A)
            );
            input.get_gamepad_button_pressed(id, GamepadButton::A)
        });

        let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
        (movements, wobbles, drop_bombs, delta_time)
    };

    // actions
    //drop a bomb
    //pub struct PlayerActions{
    //    pub bomb_pressed: bool,
    //}
    //let actions: [PlayerActions; NUM_PLAYERS as usize] = {
    //    std::array::from_fn(|i| {
    //        let id = PlayerId::try_from(i as u8).unwrap();
    //        let bomb_pressed = input.get_gamepad_button(id, GamepadButton::B);
    //        PlayerActions{bomb_pressed}
    //    }
    //    )
    //}

    // TODO: debug - this just moves one player
    //let (k_w,k_s,k_a,k_d)=(
    //    input.get_key(KeyboardKey::KeyW),
    //    input.get_key(KeyboardKey::KeyS),
    //    input.get_key(KeyboardKey::KeyA),
    //    input.get_key(KeyboardKey::KeyD));

    for (_, transform_component, player_component) in
        engine.iterate_two_components_mut::<TransformComponent, PlayerComponent>()?
    {
        let movement = movements[player_component.id as usize];

        // TODO: check which is which
        if movement.0.abs() > DZ {
            transform_component
                .translate(movement.0 * MOVEMENT_SPEED * delta_time, Direction::Left);
        }
        //if (actions[player_component.id as usize].bomb_pressed){
        //    add_bomb(engine, transform_component.position.x, transform_component.position.y)?;
        //}
    }

    Ok(())
}
