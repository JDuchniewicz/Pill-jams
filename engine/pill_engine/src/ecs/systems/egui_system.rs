#![cfg(feature = "ui")]

use crate::{ecs::EguiComponent, engine::Engine};
use pill_core::Result;

pub fn egui_system(engine: &mut Engine) -> Result<()> {
    // Hidden UI (the default in release builds) builds no UI function at all, so the egui pass
    // runs with an empty closure and draws nothing.
    let visible = engine.get_global_component::<EguiComponent>()?.is_visible();
    if !visible {
        return Ok(());
    }

    let ui = EguiComponent::get_ui(engine)?;
    let client = engine
        .get_global_component::<EguiComponent>()?
        .egui_client
        .clone();
    client.set_ui(move |ctx| ui(ctx));
    Ok(())
}
