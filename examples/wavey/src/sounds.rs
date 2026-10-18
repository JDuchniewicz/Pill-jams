use pill_engine::{define_component, define_global_component, game::*};

pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    // TODO: Add ambient sounds
    //let ambient_music = Sound::new("ambient", "audio/test_music.mp3".into());
    //let ambient_music_handle = engine.add_resource(ambient_music)?;
    // TODO: add an array of sounds and play them randomly

    //engine
    //    .build_entity(active_scene)
    //    .with_component(
    //        AudioSourceComponent::builder()
    //            .sound_type(SoundType::Sound2D)
    //            .sound(ambient_music_handle)
    //            .volume(0.05)
    //            .play_on_awake(false)
    //            .build(),
    //    )
    //    .build();

    Ok(())
}

pub fn play_sound(engine: &mut Engine) -> Result<()> {
    Ok(())
}
