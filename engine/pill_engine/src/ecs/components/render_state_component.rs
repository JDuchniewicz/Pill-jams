use crate::{
    ecs::components::{GlobalComponent, GlobalComponentStorage},
    graphics::PassTextureHandle,
};

use pill_core::PillTypeMapKey;

/// HDR → display mapping applied by the tonemap pass.
#[derive(Clone, Copy, Debug)]
pub enum Tonemap {
    /// Linear pass-through; values above 1 clip.
    None,
    /// Extended Reinhard c·(1 + c/W²)/(1 + c): `white_point` W maps to 1.0; INFINITY = plain c/(1 + c).
    Reinhard { white_point: f32 },
}

/// Engine-owned (always present, cannot be removed or constructed by games); games set the
/// public environment, IBL, fog and tonemap fields.
pub struct RenderStateComponent {
    pub(crate) boot_done: bool,
    /// Scene color target (HDR_TARGET_FORMAT) shared by scene passes; resolved by the tonemap pass.
    pub(crate) hdr_target: PassTextureHandle,
    /// Scene depth target (DEPTH_TARGET_FORMAT) shared by scene passes.
    pub(crate) depth_target: PassTextureHandle,
    /// Engine default IBL maps (diffuse, specular, BRDF LUT): used when a game-set handle is invalid.
    pub(crate) default_ibl: [PassTextureHandle; 3],
    /// Environment (equirect) image drawn behind the scene; `None` draws `background_color`.
    pub background: Option<PassTextureHandle>,
    /// `true` when a game-defined background pass (`Engine::add_background_pass`) runs before the
    /// opaque PBR pass: the scene color is then loaded instead of cleared, and the engine's own
    /// background sub-draw is skipped.
    pub(crate) has_background_pass: bool,
    /// Solid background color used when `background` is `None` (linear RGB).
    pub background_color: [f32; 3],
    pub ibl_diffuse: PassTextureHandle,
    pub ibl_specular: PassTextureHandle,
    pub ibl_brdf_lut: PassTextureHandle,
    /// Exponential-squared fog: color is mixed toward `fog_color` by 1 - exp(-density² · distance²);
    /// density 0 disables fog.
    pub fog_color: [f32; 3],
    pub fog_density: f32,
    pub tonemap: Tonemap,
}

impl RenderStateComponent {
    /// Creates the component in its pre-boot state; handles are replaced by `create_default_resources`.
    pub(crate) fn new() -> Self {
        Self {
            boot_done: false,
            hdr_target: PassTextureHandle::default(),
            depth_target: PassTextureHandle::default(),
            default_ibl: [PassTextureHandle::default(); 3],
            background: None,
            has_background_pass: false,
            background_color: [0.15, 0.15, 0.15],
            ibl_diffuse: PassTextureHandle::default(),
            ibl_specular: PassTextureHandle::default(),
            ibl_brdf_lut: PassTextureHandle::default(),
            fog_color: [0.0, 0.0, 0.0],
            fog_density: 0.0,
            tonemap: Tonemap::Reinhard {
                white_point: f32::INFINITY,
            },
        }
    }
}

impl RenderStateComponent {
    /// Scene color target (HDR_TARGET_FORMAT) for game-defined passes.
    pub fn hdr_target(&self) -> PassTextureHandle {
        self.hdr_target
    }

    /// Scene depth target (DEPTH_TARGET_FORMAT) for game-defined passes.
    pub fn depth_target(&self) -> PassTextureHandle {
        self.depth_target
    }

    /// `true` when a game-defined background pass already filled the scene color this frame.
    pub fn has_background_pass(&self) -> bool {
        self.has_background_pass
    }

    pub(crate) fn set_has_background_pass(&mut self, has_background_pass: bool) {
        self.has_background_pass = has_background_pass;
    }
}

impl PillTypeMapKey for RenderStateComponent {
    type Storage = GlobalComponentStorage<RenderStateComponent>;
}

impl GlobalComponent for RenderStateComponent {}
