use crate::ecs::components::{GlobalComponent, GlobalComponentStorage};

use pill_core::PillTypeMapKey;

use pill_core::Result;

#[readonly::make]
pub struct TimeComponent {
    #[readonly]
    pub time: f32, // Time elapsed from the start of the engine/game in seconds
    #[readonly]
    pub delta_time: f32, // Time of last frame in seconds
}

impl Default for TimeComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeComponent {
    pub fn new() -> Self {
        Self {
            time: 0.0,
            delta_time: 0.0,
        }
    }

    /// Replaces this frame's delta time. Game systems use this for effects such as slow motion;
    /// every system that runs after the caller sees the new value.
    pub fn set_delta_time(&mut self, delta_time: f32) {
        self.delta_time = delta_time.max(0.0);
    }

    pub(crate) fn update(&mut self, delta_time: f32) -> Result<()> {
        self.time += delta_time / 1000.0;
        self.delta_time = delta_time / 1000.0;

        Ok(())
    }
}

impl PillTypeMapKey for TimeComponent {
    type Storage = GlobalComponentStorage<TimeComponent>;
}

impl GlobalComponent for TimeComponent {}
