use crate::{
    ecs::{
        AudioManagerComponent, Component, ComponentStorage, DeferredUpdateComponent,
        DeferredUpdateComponentRequest, DeferredUpdateManagerPointer, EntityHandle, SceneHandle,
        SoundType,
    },
    engine::Engine,
    resources::{Sound, SoundHandle},
};

use pill_core::{
    get_enum_variant_type_name, get_type_name, warn, LogContext, PillStyle, PillTypeMapKey,
};

use pill_core::{ErrorContext, Result};

const DEFERRED_REQUEST_VARIANT_SET_SOUND: usize = 0;
const DEFERRED_REQUEST_VARIANT_SET_VOLUME: usize = 1;
const DEFERRED_REQUEST_VARIANT_PLAY_SOUND: usize = 2;
const DEFERRED_REQUEST_VARIANT_PAUSE_SOUND: usize = 3;
const DEFERRED_REQUEST_VARIANT_STOP_SOUND: usize = 4;

// --- Builder ---

pub struct AudioSourceComponentBuilder {
    component: AudioSourceComponent,
}

impl AudioSourceComponentBuilder {
    pub fn default() -> Self {
        Self {
            component: AudioSourceComponent::new(),
        }
    }

    pub fn sound_type(mut self, sound_type: SoundType) -> Self {
        self.component.sound_type = sound_type;
        self
    }

    pub fn sound(mut self, sound_handle: SoundHandle) -> Self {
        self.component.sound_handle = Some(sound_handle);
        self
    }

    pub fn volume(mut self, volume: f32) -> Self {
        self.component.volume = volume;
        self
    }

    pub fn play_on_awake(mut self, play: bool) -> Self {
        self.component.play_on_awake = play;
        self
    }

    pub fn build(self) -> AudioSourceComponent {
        self.component
    }
}

// --- Audio Source Component ---

#[readonly::make]
pub struct AudioSourceComponent {
    #[readonly]
    pub sound_type: SoundType,
    #[readonly]
    pub volume: f32,
    #[readonly]
    pub play_on_awake: bool,
    #[readonly]
    pub sound_handle: Option<SoundHandle>,
    #[readonly]
    pub is_playing: bool,
    pub(crate) sink_handle: Option<usize>,

    entity_handle: Option<EntityHandle>,
    scene_handle: Option<SceneHandle>,
    deferred_update_manager: Option<DeferredUpdateManagerPointer>,
}

impl Default for AudioSourceComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioSourceComponent {
    pub fn builder() -> AudioSourceComponentBuilder {
        AudioSourceComponentBuilder::default()
    }

    pub fn new() -> Self {
        Self {
            sound_type: SoundType::Sound3D,
            volume: 1.0,
            play_on_awake: false,
            sound_handle: None,
            sink_handle: None,
            is_playing: false,

            entity_handle: None,
            scene_handle: None,
            deferred_update_manager: None,
        }
    }

    pub fn set_sound(&mut self, sound_handle: SoundHandle) {
        self.sound_handle = Some(sound_handle);
        if self.sink_handle.is_some() {
            self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_SET_SOUND);
        }
    }

    pub fn remove_sound(&mut self) {
        self.sound_handle = None;
        if self.sink_handle.is_some() {
            self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_STOP_SOUND);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        if self.sink_handle.is_some() {
            self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_SET_VOLUME);
        }
    }

    pub fn play(&mut self) {
        if self.sound_handle.is_some() {
            self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_PLAY_SOUND);
        }
    }

    pub fn pause(&mut self) {
        if self.sink_handle.is_some() {
            self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_PAUSE_SOUND);
        }
    }

    pub fn stop(&mut self) {
        // Queue stop even if the sink is not assigned yet: an earlier queued play
        // can assign one before this request is processed.
        self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_STOP_SOUND);
    }

    // Give the handle (index) back to the AudioManagerComponent
    pub(crate) fn return_sink(&mut self) -> Option<usize> {
        self.is_playing = false;
        self.sink_handle.take()
    }

    // Stop and release the sink exactly once. Deferred requests can outlive a
    // completed one-shot, so do not assume a handle is still assigned.
    fn stop_playing(&mut self, engine: &mut Engine) -> Result<()> {
        self.is_playing = false;
        let Some(sink_handle) = self.sink_handle.take() else {
            return Ok(());
        };

        let audio_manager = engine.get_global_component_mut::<AudioManagerComponent>()?;
        match self.sound_type {
            SoundType::Sound3D => audio_manager.get_spatial_sink(sink_handle).stop(),
            SoundType::Sound2D => audio_manager.get_ambient_sink(sink_handle).stop(),
        }
        audio_manager.return_sink(sink_handle, &self.sound_type);
        Ok(())
    }

    // Post deferred update request
    fn post_deferred_update_request(&mut self, request_variant: usize) {
        if let Some(manager) = self.deferred_update_manager.as_mut() {
            let entity_handle = self.entity_handle.expect(
                "Critical: Cannot post deferred update request. No EntityHandle set in Component",
            );
            let scene_handle = self.scene_handle.expect(
                "Critical: Cannot post deferred update request. No SceneHandle set in Component",
            );
            let request = DeferredUpdateComponentRequest::<AudioSourceComponent>::new(
                entity_handle,
                scene_handle,
                request_variant,
            );
            manager.post_update_request(request);
        }
    }
}

impl PillTypeMapKey for AudioSourceComponent {
    type Storage = ComponentStorage<AudioSourceComponent>;
}

impl Component for AudioSourceComponent {
    fn initialize(&mut self, engine: &mut Engine) -> Result<()> {
        // This component is using DeferredUpdateSystem so keep DeferredUpdateManager
        let deferred_update_component = engine
            .get_global_component_mut::<DeferredUpdateComponent>()
            .expect("Critical: No DeferredUpdateComponent");
        self.deferred_update_manager =
            Some(deferred_update_component.borrow_deferred_update_manager());

        // Check if sound handle is valid
        if let Some(handle) = &self.sound_handle {
            engine.get_resource::<Sound>(handle).context(format!(
                "Creating {} {} failed",
                "Component".general_object_style(),
                get_type_name::<Self>().specific_object_style()
            ))?;
        }

        Ok(())
    }

    fn pass_handles(&mut self, self_scene_handle: SceneHandle, self_entity_handle: EntityHandle) {
        self.scene_handle = Some(self_scene_handle);
        self.entity_handle = Some(self_entity_handle);

        // Play on awake
        if self.play_on_awake {
            self.play();
        }
    }

    fn deferred_update(&mut self, engine: &mut Engine, request: usize) -> Result<()> {
        match request {
            DEFERRED_REQUEST_VARIANT_SET_SOUND => {
                if let Some(handle) = self.sound_handle {
                    engine.get_resource::<Sound>(&handle).context(format!(
                        "Setting {} {} failed",
                        "Component".general_object_style(),
                        "Sound".specific_object_style()
                    ))?;
                }
                // A new sound replaces the current one; it does not append to it.
                self.stop_playing(engine)?;
            }
            DEFERRED_REQUEST_VARIANT_SET_VOLUME => {
                if let Some(handle) = self.sink_handle {
                    let audio_manager = engine.get_global_component::<AudioManagerComponent>()?;
                    match self.sound_type {
                        SoundType::Sound3D => audio_manager
                            .get_spatial_sink(handle)
                            .set_volume(self.volume),
                        SoundType::Sound2D => audio_manager
                            .get_ambient_sink(handle)
                            .set_volume(self.volume),
                    }
                }
            }
            DEFERRED_REQUEST_VARIANT_PLAY_SOUND => {
                let Some(sound_handle) = self.sound_handle else {
                    return Ok(());
                };

                // Replaying an occupied source must not leak the old sink.
                self.stop_playing(engine)?;

                let sound_data = engine
                    .get_resource::<Sound>(&sound_handle)?
                    .sound_data
                    .as_ref()
                    .expect("Initialized Sound must contain sound data")
                    .get_source_sound();

                let audio_manager = engine.get_global_component_mut::<AudioManagerComponent>()?;
                if let Some(handle) = audio_manager.get_free_sink_handle(&self.sound_type) {
                    match self.sound_type {
                        SoundType::Sound2D => {
                            let sink = audio_manager.get_ambient_sink(handle);
                            sink.append(sound_data);
                            sink.set_volume(self.volume);
                            sink.play();
                        }
                        SoundType::Sound3D => {
                            let sink = audio_manager.get_spatial_sink(handle);
                            sink.append(sound_data);
                            sink.set_volume(self.volume);
                            sink.play();
                        }
                    }
                    self.sink_handle = Some(handle);
                    self.is_playing = true;
                } else {
                    self.is_playing = false;
                    warn!(LogContext::ECS => "Cannot play sound, max concurrent {} sound count reached", get_enum_variant_type_name(&self.sound_type));
                }
            }
            DEFERRED_REQUEST_VARIANT_PAUSE_SOUND => {
                if let Some(handle) = self.sink_handle {
                    let audio_manager = engine.get_global_component::<AudioManagerComponent>()?;
                    match self.sound_type {
                        SoundType::Sound3D => audio_manager.get_spatial_sink(handle).pause(),
                        SoundType::Sound2D => audio_manager.get_ambient_sink(handle).pause(),
                    }
                    self.is_playing = false;
                }
            }
            DEFERRED_REQUEST_VARIANT_STOP_SOUND => {
                self.stop_playing(engine)?;
            }
            _ => {
                panic!("Critical: Processing deferred update request with value {} in {} failed. Handling is not implemented", request, get_type_name::<Self>().specific_object_style());
            }
        }
        Ok(())
    }

    fn destroy(
        &mut self,
        engine: &mut Engine,
        _self_scene_handle: SceneHandle,
        _self_entity_handle: EntityHandle,
    ) -> Result<()> {
        if self.sink_handle.is_some() {
            self.stop_playing(engine)?;
        }

        Ok(())
    }
}
