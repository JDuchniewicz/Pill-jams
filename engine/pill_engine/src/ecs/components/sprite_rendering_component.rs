use crate::{
    ecs::{Component, ComponentStorage, EntityHandle, SceneHandle},
    engine::Engine,
    resources::{Texture, TextureHandle},
};

use pill_core::{get_type_name, Color, ErrorContext, PillStyle, PillTypeMapKey, Result, Vector2f};

// --- Builder ---

pub struct SpriteRenderingComponentBuilder {
    component: SpriteRenderingComponent,
}

impl SpriteRenderingComponentBuilder {
    pub fn default() -> Self {
        Self {
            component: SpriteRenderingComponent::new(),
        }
    }

    pub fn texture(mut self, texture_handle: &TextureHandle) -> Self {
        self.component.texture_handle = Some(*texture_handle);
        self
    }

    /// Quad size in world units, before the entity's transform scale.
    pub fn size(mut self, size: Vector2f) -> Self {
        self.component.size = size;
        self
    }

    /// Linear RGB tint multiplied with the sampled texel.
    pub fn color(mut self, color: Color) -> Self {
        self.component.set_color(color);
        self
    }

    /// Multiplies the texel alpha (1 = opaque).
    pub fn alpha(mut self, alpha: f32) -> Self {
        self.component.color[3] = alpha.max(0.0);
        self
    }

    /// Draw order; lower values are drawn first (behind).
    pub fn order(mut self, order: i32) -> Self {
        self.component.order = order;
        self
    }

    /// `false` disables depth testing, so the sprite is drawn over all scene geometry.
    pub fn depth_test(mut self, depth_test: bool) -> Self {
        self.component.depth_test = depth_test;
        self
    }

    pub fn flip_x(mut self, flip: bool) -> Self {
        self.component.flip_x = flip;
        self
    }

    pub fn flip_y(mut self, flip: bool) -> Self {
        self.component.flip_y = flip;
        self
    }

    /// Darkens the part of the sprite below a world-space line (`height`, world Y) by `factor`:
    /// `submerge(0.5, water_y)` draws everything below `water_y` at half brightness.
    /// `factor = 1.0` disables the effect.
    pub fn submerge(mut self, factor: f32, height: f32) -> Self {
        self.component.submerge_factor = factor.max(0.0);
        self.component.submerge_height = height;
        self
    }

    pub fn build(self) -> SpriteRenderingComponent {
        self.component
    }
}

// --- Sprite Rendering Component ---

/// Draws a textured quad (a sprite) with the built-in `PassSprites`.
///
/// The quad is centered on the entity's `TransformComponent::position`, faces the camera and is
/// `size * transform.scale` world units wide/tall. Sprites are drawn after the opaque scene,
/// sorted by `order`, with alpha blending; `depth_test = false` keeps them on top of everything.
#[readonly::make]
pub struct SpriteRenderingComponent {
    #[readonly]
    pub texture_handle: Option<TextureHandle>,
    #[readonly]
    pub size: Vector2f,
    #[readonly]
    pub color: [f32; 4],
    #[readonly]
    pub order: i32,
    #[readonly]
    pub depth_test: bool,
    #[readonly]
    pub flip_x: bool,
    #[readonly]
    pub flip_y: bool,
    /// World-space Y of the waterline; fragments below it are multiplied by `submerge_factor`.
    #[readonly]
    pub submerge_height: f32,
    /// Brightness multiplier applied below `submerge_height` (1.0 = no effect).
    #[readonly]
    pub submerge_factor: f32,
}

impl Default for SpriteRenderingComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl SpriteRenderingComponent {
    pub fn builder() -> SpriteRenderingComponentBuilder {
        SpriteRenderingComponentBuilder::default()
    }

    pub fn new() -> Self {
        Self {
            texture_handle: None,
            size: Vector2f::new(1.0, 1.0),
            color: [1.0, 1.0, 1.0, 1.0],
            order: 0,
            depth_test: true,
            flip_x: false,
            flip_y: false,
            submerge_height: 0.0,
            submerge_factor: 1.0,
        }
    }

    pub fn set_texture(&mut self, texture_handle: &TextureHandle) {
        self.texture_handle = Some(*texture_handle);
    }

    pub fn set_size(&mut self, size: Vector2f) {
        self.size = size;
    }

    /// Sets the tint, keeping the current alpha.
    pub fn set_color(&mut self, color: Color) {
        self.color = [color.x, color.y, color.z, self.color[3]];
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        self.color[3] = alpha.max(0.0);
    }

    pub fn set_order(&mut self, order: i32) {
        self.order = order;
    }

    pub fn set_depth_test(&mut self, depth_test: bool) {
        self.depth_test = depth_test;
    }

    pub fn set_flip(&mut self, flip_x: bool, flip_y: bool) {
        self.flip_x = flip_x;
        self.flip_y = flip_y;
    }

    /// Sets the waterline (world Y) and the brightness factor applied below it.
    pub fn set_submerge(&mut self, factor: f32, height: f32) {
        self.submerge_factor = factor.max(0.0);
        self.submerge_height = height;
    }

    /// Moves the waterline; keeps the current factor.
    pub fn set_submerge_height(&mut self, height: f32) {
        self.submerge_height = height;
    }
}

impl PillTypeMapKey for SpriteRenderingComponent {
    type Storage = ComponentStorage<SpriteRenderingComponent>;
}

impl Component for SpriteRenderingComponent {
    fn initialize(&mut self, engine: &mut Engine) -> Result<()> {
        // Validate the texture handle early; the pass skips sprites whose texture is missing
        if let Some(handle) = &self.texture_handle {
            engine.get_resource::<Texture>(handle).context(format!(
                "Creating {} {} failed",
                "Component".general_object_style(),
                get_type_name::<Self>().specific_object_style()
            ))?;
        }

        Ok(())
    }

    fn pass_handles(&mut self, _self_scene_handle: SceneHandle, _self_entity_handle: EntityHandle) {
    }
}
