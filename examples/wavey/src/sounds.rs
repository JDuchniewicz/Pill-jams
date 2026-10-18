use pill_engine::{define_component, define_global_component, game::*};

// TODO: download the ambient music and play it
// TODO:LIST
// audio blares
// audio music
// collision in ass and rumble
// containers?

const AUDIO_LENGTH: f32 = 5.0;

define_component!(SingleShotAudioComponent{
    active: bool,
    timeout: f32,
    elapsed: f32,
    tracks: Vec<SoundHandle>, // TODO: could be kept in a global component
    chosen_track: usize,
});

pub fn start(
    engine: &mut Engine,
    active_scene: SceneHandle,
    player_handles: Vec<EntityHandle>,
) -> Result<()> {
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
    engine.register_component::<SingleShotAudioComponent>(active_scene)?;

    let blare_1 = Sound::new("blare_1", "audio/blare_1.wav".into());
    let blare_1_handle = engine.add_resource(blare_1)?;
    let blare_2 = Sound::new("blare_2", "audio/blare_2.wav".into());
    let blare_2_handle = engine.add_resource(blare_2)?;
    let blare_3 = Sound::new("blare_3", "audio/blare_3.wav".into());
    let blare_3_handle = engine.add_resource(blare_3)?;

    for handle in player_handles {
        engine.add_component_to_entity(
            active_scene,
            handle,
            SingleShotAudioComponent {
                active: false,
                timeout: AUDIO_LENGTH,
                elapsed: 0.0,
                tracks: Vec::from([blare_1_handle, blare_2_handle, blare_3_handle]),
                chosen_track: 0,
            },
        )?;
    }

    engine.add_system("single_shot_audio_system", single_shot_audio_system)?;

    Ok(())
}

pub fn schedule_audio(component: &mut SingleShotAudioComponent) -> Result<()> {
    // don't restart those in progress
    if component.active {
        return Ok(());
    }

    component.active = true;
    component.timeout = AUDIO_LENGTH;
    component.elapsed = 0.0;
    component.chosen_track = rand::random_range(0..component.tracks.len());
    Ok(())
}

pub fn single_shot_audio_system(engine: &mut Engine) -> Result<()> {
    // process all tracks, find new scheduled
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    for (_, audio_component, audio_source) in
        engine.iterate_two_components_mut::<SingleShotAudioComponent, AudioSourceComponent>()?
    {
        if audio_component.active {
            // start the new track
            if audio_component.elapsed == 0.0 {
                audio_source.set_sound(audio_component.tracks[audio_component.chosen_track]);
                audio_source.play();
                audio_component.elapsed += delta_time;
            } else if audio_component.elapsed >= audio_component.timeout {
                audio_source.stop();
                audio_component.active = false;
            } else {
                // update time
                audio_component.elapsed += delta_time;
            }
        }
    }
    Ok(())
}
