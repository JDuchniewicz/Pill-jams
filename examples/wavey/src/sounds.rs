use crate::{player::PlayerComponent, water_floating::WaterFloatingComponent};
use pill_engine::{define_component, game::*};

// TODO:LIST
// collision in ass and rumble
// containers?

const BLARE_LENGTH: f32 = 5.0;

define_component!(SingleShotAudioComponent{
    active: bool,
    timeout: f32,
    elapsed: f32,
    tracks: Vec<SoundHandle>, // TODO: could be kept in a global component
    chosen_track: usize,
});

define_component!(LoopingAudioComponent{
    timeouts: Vec<f32>,
    elapsed: f32,
    tracks: Vec<SoundHandle>,
    chosen_track: usize,
});

pub fn start(
    engine: &mut Engine,
    active_scene: SceneHandle,
    player_handles: Vec<EntityHandle>,
) -> Result<()> {
    let fast_mode = std::env::var("FAST_MODE")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    engine.register_component::<SingleShotAudioComponent>(active_scene)?;
    engine.register_component::<LoopingAudioComponent>(active_scene)?;

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
                timeout: BLARE_LENGTH,
                elapsed: 0.0,
                tracks: Vec::from([blare_1_handle, blare_2_handle, blare_3_handle]),
                chosen_track: rand::random_range(0..3), // TODO: shoudl be settable later
            },
        )?;
    }

    engine.add_system("single_shot_audio_system", single_shot_audio_system)?;

    if !fast_mode {
        let ambient_paths = [
            ("ambient_1", "audio/hejho.mp3"),
            ("ambient_2", "audio/slop.mp3"),
            ("ambient_3", "audio/spanish.mp3"),
        ];

        let mut tracks = Vec::new();
        let mut timeouts = Vec::new();

        for (name, path) in ambient_paths {
            let mut sound = Sound::new(name, path.into());
            sound.initialize(engine)?;

            let duration = sound.duration().map(|d| d.as_secs_f32()).unwrap_or(0.0);

            tracks.push(engine.add_resource(sound)?);
            timeouts.push(duration);
        }

        engine
            .build_entity(active_scene)
            .with_component(LoopingAudioComponent {
                timeouts,
                elapsed: 0.0,
                tracks,
                chosen_track: 0,
            })
            .with_component(
                AudioSourceComponent::builder()
                    .sound_type(SoundType::Sound2D)
                    .volume(0.15)
                    .play_on_awake(false)
                    .build(),
            )
            .build();

        engine.add_system("looping_audio_system", looping_audio_system)?;
    }

    Ok(())
}

pub fn schedule_blare(component: &mut SingleShotAudioComponent) -> Result<()> {
    // don't restart those in progress
    if component.active {
        return Ok(());
    }

    component.active = true;
    component.timeout = BLARE_LENGTH;
    component.elapsed = 0.0;
    component.chosen_track = rand::random_range(0..component.tracks.len());
    Ok(())
}

pub fn single_shot_audio_system(engine: &mut Engine) -> Result<()> {
    // process all tracks, find new scheduled
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    for (_, audio_component, audio_source, floating_component) in
        engine.iterate_three_components_mut::<SingleShotAudioComponent, AudioSourceComponent, WaterFloatingComponent>()?
    {
        if audio_component.active {
            // start the new track
            if audio_component.elapsed == 0.0 {
                audio_source.set_sound(audio_component.tracks[audio_component.chosen_track]);
                // If player under the water, lower the pitch
                let rand = if floating_component.is_fully_submerged() {
                    rand::random_range(0.75..0.85)
                } else {
                    rand::random_range(0.90..1.15)
                };
                audio_source.set_speed(rand);
                audio_component.timeout = BLARE_LENGTH * (1.0 / rand);
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

pub fn looping_audio_system(engine: &mut Engine) -> Result<()> {
    // process all tracks, find new scheduled
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    for (_, audio_component, audio_source) in
        engine.iterate_two_components_mut::<LoopingAudioComponent, AudioSourceComponent>()?
    {
        // start the new track
        if audio_component.elapsed == 0.0 {
            audio_source.set_sound(audio_component.tracks[audio_component.chosen_track]);
            audio_source.play();
            audio_component.elapsed += delta_time;
        } else if audio_component.elapsed >= audio_component.timeouts[audio_component.chosen_track]
        {
            audio_source.stop();
            // loop the tracks
            if audio_component.chosen_track == audio_component.tracks.len() {
                audio_component.chosen_track = 0;
            } else {
                audio_component.chosen_track += 1;
            }
            audio_component.elapsed = 0.0;
        } else {
            // update time
            audio_component.elapsed += delta_time;
        }
    }
    Ok(())
}
