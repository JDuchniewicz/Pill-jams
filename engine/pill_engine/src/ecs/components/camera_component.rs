use crate::ecs::{Component, ComponentStorage};

use pill_core::PillTypeMapKey;

use std::ops::Range;

pub enum CameraAspectRatio {
    Automatic(f32),
    Manual(f32),
}

impl CameraAspectRatio {
    pub fn get_value(&self) -> f32 {
        match self {
            CameraAspectRatio::Automatic(v) => *v,
            CameraAspectRatio::Manual(v) => *v,
        }
    }
}

/// Projection used by a camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraProjection {
    /// Perspective projection; `CameraComponent::fov` is the vertical field of view in degrees.
    Perspective,
    /// Orthographic projection; `vertical_size` is the visible height in world units.
    Orthographic { vertical_size: f32 },
}

impl Default for CameraProjection {
    fn default() -> Self {
        Self::Perspective
    }
}

// --- Builder ---

pub struct CameraComponentBuilder {
    component: CameraComponent,
}

impl CameraComponentBuilder {
    pub fn default() -> Self {
        Self {
            component: CameraComponent::new(),
        }
    }

    pub fn aspect(mut self, aspect: CameraAspectRatio) -> Self {
        self.component.aspect = aspect;
        self
    }

    pub fn fov(mut self, fov: f32) -> Self {
        self.component.fov = fov;
        self
    }

    /// Sets the projection explicitly (see [`CameraProjection`]).
    pub fn projection(mut self, projection: CameraProjection) -> Self {
        self.component.projection = projection;
        self
    }

    /// Convenience for `projection(CameraProjection::Perspective)`.
    pub fn perspective(mut self) -> Self {
        self.component.projection = CameraProjection::Perspective;
        self
    }

    /// Convenience for `projection(CameraProjection::Orthographic { .. })`.
    /// `vertical_size` is the visible height in world units.
    pub fn orthographic(mut self, vertical_size: f32) -> Self {
        self.component.projection = CameraProjection::Orthographic { vertical_size };
        self
    }

    pub fn range(mut self, range: Range<f32>) -> Self {
        self.component.range = range;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.component.enabled = enabled;
        self
    }

    pub fn build(self) -> CameraComponent {
        self.component
    }
}

// --- Camera Component ---

pub struct CameraComponent {
    pub aspect: CameraAspectRatio,
    pub fov: f32,
    pub projection: CameraProjection,
    pub range: Range<f32>,
    pub enabled: bool,
}

impl Default for CameraComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraComponent {
    pub fn builder() -> CameraComponentBuilder {
        CameraComponentBuilder::default()
    }

    pub fn new() -> Self {
        Self {
            aspect: CameraAspectRatio::Automatic(1.0),
            fov: 60.0,
            projection: CameraProjection::Perspective,
            range: 0.1..100.0,
            enabled: false,
        }
    }
}

impl PillTypeMapKey for CameraComponent {
    type Storage = ComponentStorage<CameraComponent>;
}

impl Component for CameraComponent {}
