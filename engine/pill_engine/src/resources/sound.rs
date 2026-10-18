use crate::{
    ecs::AudioSourceComponent,
    engine::Engine,
    resources::{Resource, ResourceStorage},
};

use pill_core::{get_type_name, EngineError, PillSlotMapKey, PillStyle, PillTypeMapKey};

use pill_core::{ErrorContext, Result};
use rodio::{Decoder, Source};
use std::{
    fs::File,
    io::{Cursor, Read},
    path::PathBuf,
    time::Duration,
};

pill_core::define_new_pill_slotmap_key! {
    pub struct SoundHandle;
}

#[readonly::make]
pub struct Sound {
    #[readonly]
    pub name: String,
    #[readonly]
    pub path: PathBuf,
    pub(crate) sound_data: Option<SoundData>,
}

impl Sound {
    pub fn new(name: &str, path: PathBuf) -> Self {
        Self {
            name: name.to_string(),
            path,
            sound_data: None,
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        let sound_data = self.sound_data.as_ref()?;
        Some(sound_data.get_duration())
    }
}

impl PillTypeMapKey for Sound {
    type Storage = ResourceStorage<Sound>;
}

impl Resource for Sound {
    type Handle = SoundHandle;

    fn initialize(&mut self, engine: &mut Engine) -> Result<()> {
        let error_message = format!(
            "Initializing {} {} failed",
            "Resource".general_object_style(),
            get_type_name::<Self>().specific_object_style()
        );

        // Check if path to asset is correct
        let resource_file_path = engine.game_resources_directory_path.join(&self.path);
        pill_core::validate_asset_path(&resource_file_path, &["mp3", "wav"])
            .context(error_message.clone())?;

        // Create sound data
        let sound_data = SoundData::new(&resource_file_path).context(error_message.clone())?;
        self.sound_data = Some(sound_data);

        Ok(())
    }

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn destroy<H: PillSlotMapKey>(&mut self, engine: &mut Engine, self_handle: H) -> Result<()> {
        // Find audio source components that use this sound and update them
        for (_scene_handle, scene) in engine.scene_manager.scenes.iter_mut() {
            for (_entity_handle, audio_source_component) in
                scene.get_one_component_iterator_mut::<AudioSourceComponent>()?
            {
                if let Some(sound_handle) = audio_source_component.sound_handle {
                    // If audio source component has handle to this sound
                    if sound_handle.data() == self_handle.data() {
                        audio_source_component.remove_sound();
                    }
                }
            }
        }

        Ok(())
    }
}

pub struct SoundData {
    pub(crate) source_buffer: Vec<u8>,
}

impl SoundData {
    pub fn new(path: &PathBuf) -> Result<Self> {
        // Open sound file
        let mut sound_file = match File::open(path) {
            Err(_err) => {
                return Err(EngineError::InvalidAssetPath(
                    path.clone().into_os_string().into_string().unwrap(),
                )
                .into())
            }
            file => file?,
        };

        // Read bytes to vector
        let mut sound_data = Vec::new();
        sound_file.read_to_end(&mut sound_data).unwrap();

        // Create SoundData
        let sound_data = SoundData {
            source_buffer: sound_data,
        };

        Ok(sound_data)
    }

    pub fn get_source_sound(&self) -> Decoder<Cursor<Vec<u8>>> {
        let mut sound_source = Vec::<u8>::new();

        // Read bytes from the buffer
        for buffer in self.source_buffer.iter() {
            sound_source.push(*buffer);
        }

        //        println!(
        //            "Decoding sound: {} bytes, header: {:02X?}",
        //            self.source_buffer.len(),
        //            &self.source_buffer[..self.source_buffer.len().min(16)]
        //        );
        //
        // Return decoded bytes as the sound, which can be played
        Decoder::new(Cursor::new(sound_source)).unwrap()
    }

    pub fn get_duration(&self) -> Duration {
        let decoder = self.get_source_sound();

        let sample_rate = decoder.sample_rate() as u64;
        let channels = decoder.channels() as u64;

        if let Some(duration) = decoder.total_duration() {
            return duration;
        }

        let sample_count = decoder.count() as u64;

        Duration::from_secs_f64(sample_count as f64 / (sample_rate * channels) as f64)
    }
}
