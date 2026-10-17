use crate::config::{
    DEPTH_TARGET_FORMAT, HDR_TARGET_FORMAT, MAX_SPRITES, SPRITE_FRAGMENT_SHADER,
    SPRITE_SUBMERGED_FRAGMENT_SHADER, SPRITE_SUBMERGED_VERTEX_SHADER, SPRITE_VERTEX_SHADER,
};
use crate::ecs::{SpriteRenderingComponent, TransformComponent};
use crate::graphics::{
    camera_view_projection, BufferDesc, FrameTarget, Pass, PassTextureHandle, PillRenderer,
    Pipeline, PipelineDesc, ShaderDesc, WorldQuery,
};
use crate::resources::TextureHandle;

use pill_core::{Result, Vector3f};

/// Mirrors `Camera` in sprite_vertex.hlsl: view-projection plus the camera's world-space basis,
/// so the CPU never has to build a per-sprite matrix.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteCameraUniform {
    view_projection: [[f32; 4]; 4],
    camera_right: [f32; 4],
    camera_up: [f32; 4],
}

/// One sprite instance: quad center, camera-plane quad axes, tint, UV flips, depth-test flag
/// and the waterline (world Y + brightness factor below it).
///
/// The axes already have the sprite size, the transform scale and the transform rotation baked
/// in (projected onto the camera plane), so the vertex shader only has to offset each corner.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SpriteInstance {
    center: [f32; 3],   // offset 0
    axis_x: [f32; 2],   // offset 12: camera-plane X axis, in world units
    axis_y: [f32; 2],   // offset 20: camera-plane Y axis, in world units
    color: [f32; 4],    // offset 28
    uv_flip: [f32; 2],  // offset 44
    depth_test: f32,    // offset 52
    submerge: [f32; 2], // offset 56: x = waterline world Y, y = factor below it
}

// The attribute offsets below are hand-written, so pin the layout to them.
const _: () = assert!(std::mem::size_of::<SpriteInstance>() == 64);

impl SpriteInstance {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SpriteInstance>() as wgpu::BufferAddress,
            // One instance per sprite; the six quad vertices come from SV_VertexID.
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 20,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 28,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 44,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 52,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 56,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// One queued sprite draw: sort key, texture and the CPU-side data needed to build the GPU
/// instance once the active camera is known.
struct SpriteDraw {
    order: i32,
    texture_handle: TextureHandle,
    center: [f32; 3],
    /// Entity-local X axis (`half_size.x` long) rotated into world space.
    axis_x_world: Vector3f,
    /// Entity-local Y axis (`half_size.y` long) rotated into world space.
    axis_y_world: Vector3f,
    color: [f32; 4],
    uv_flip: [f32; 2],
    depth_test: f32,
    submerge: [f32; 2],
}

impl SpriteDraw {
    /// Builds the GPU instance for this draw.
    ///
    /// Sprites are billboards, so the rotated axes are projected onto the camera plane; the
    /// component along the view direction would only move the quad towards/away from the camera.
    fn instance(&self, camera_right: Vector3f, camera_up: Vector3f) -> SpriteInstance {
        SpriteInstance {
            center: self.center,
            axis_x: [
                self.axis_x_world.dot(camera_right),
                self.axis_x_world.dot(camera_up),
            ],
            axis_y: [
                self.axis_y_world.dot(camera_right),
                self.axis_y_world.dot(camera_up),
            ],
            color: self.color,
            uv_flip: self.uv_flip,
            depth_test: self.depth_test,
            submerge: self.submerge,
        }
    }
}

struct SpritesState {
    pipeline: Pipeline,
    /// Variant used by sprites with a waterline (`submerge_factor < 1.0`).
    pipeline_submerged: Pipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
}

/// Draws entities that own a `SpriteRenderingComponent`.
///
/// Runs after the engine's opaque PBR pass and after game-defined passes, before the primitives
/// pass and tonemapping, so sprites composite into the HDR scene. Sprites are sorted by
/// `SpriteRenderingComponent::order` (ascending) and blended; depth testing is per sprite.
pub struct PassSprites {
    color_target: PassTextureHandle,
    depth_target: PassTextureHandle,
    state: Option<SpritesState>,
    sprites: Vec<SpriteDraw>,
    /// Bind group per unique texture, rebuilt every frame (textures can be hot-reloaded).
    texture_bind_groups: Vec<(TextureHandle, wgpu::BindGroup)>,
    capacity_warned: bool,
    missing_texture_warned: bool,
}

impl PassSprites {
    pub fn new(color_target: PassTextureHandle, depth_target: PassTextureHandle) -> Self {
        Self {
            color_target,
            depth_target,
            state: None,
            sprites: Vec::with_capacity(MAX_SPRITES),
            texture_bind_groups: Vec::new(),
            capacity_warned: false,
            missing_texture_warned: false,
        }
    }

    /// Builds a sprite pipeline for the given shader pair; both variants share the same layout.
    fn create_pipeline(
        renderer: &mut dyn PillRenderer,
        label: &str,
        vs_source: &'static str,
        ps_source: &'static str,
    ) -> Result<Pipeline> {
        let bind_groups = vec![
            // Camera uniform
            vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            // Sprite texture + sampler
            vec![
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        ];

        renderer.create_pipeline(PipelineDesc {
            label: Some(label),
            vs: ShaderDesc {
                source: vs_source,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: ps_source,
                entry_func: "fs_main",
            },
            vertex_buffers: &[SpriteInstance::layout()],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                // Sprites are usually cut-out textures, so blend them in the HDR scene.
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_TARGET_FORMAT,
                // Sprites never occlude scene geometry; `depth_test = false` sprites are pushed
                // to the nearest depth in the vertex shader instead.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
                unclipped_depth: false,
            },
        })
    }
}

impl Pass for PassSprites {
    fn get_label(&self) -> &str {
        "pass_sprites"
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let pipeline = Self::create_pipeline(
            renderer,
            "pass_sprites",
            SPRITE_VERTEX_SHADER,
            SPRITE_FRAGMENT_SHADER,
        )?;
        // Sprites with a waterline use a variant that darkens the submerged part
        let pipeline_submerged = Self::create_pipeline(
            renderer,
            "pass_sprites_submerged",
            SPRITE_SUBMERGED_VERTEX_SHADER,
            SPRITE_SUBMERGED_FRAGMENT_SHADER,
        )?;

        let camera_buffer = renderer.create_buffer(BufferDesc {
            label: Some("pass_sprites_camera"),
            byte_size: std::mem::size_of::<SpriteCameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;

        let camera_bind_group =
            renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("pass_sprites_camera_bind_group"),
                    layout: &pipeline.bind_group_layouts[0],
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_buffer.as_entire_binding(),
                    }],
                });

        let instance_buffer = renderer.create_buffer(BufferDesc {
            label: Some("pass_sprites_instances"),
            byte_size: (MAX_SPRITES * std::mem::size_of::<SpriteInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        })?;

        self.state = Some(SpritesState {
            pipeline,
            pipeline_submerged,
            camera_buffer,
            camera_bind_group,
            instance_buffer,
        });
        Ok(())
    }

    fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        renderer: &mut dyn PillRenderer,
        _frame_target: &FrameTarget,
        world: &WorldQuery<'_>,
    ) -> Result<()> {
        // A scene without sprite components simply has nothing to draw.
        let Ok(sprite_components) = world.query::<SpriteRenderingComponent>() else {
            return Ok(());
        };
        let transforms = world.query::<TransformComponent>()?;

        // Gather the draw list: visible sprites with a valid texture, sorted back to front.
        self.sprites.clear();
        for (slot_transform, slot_sprite) in
            transforms.data.iter().zip(sprite_components.data.iter())
        {
            let (Some(transform), Some(sprite)) = (slot_transform, slot_sprite) else {
                continue;
            };
            let Some(texture_handle) = sprite.texture_handle else {
                continue;
            };
            let half_size = [
                sprite.size.x * transform.scale.x * 0.5,
                sprite.size.y * transform.scale.y * 0.5,
            ];
            if !(half_size[0] > 0.0 && half_size[1] > 0.0) {
                continue;
            }
            if self.sprites.len() == MAX_SPRITES {
                if !self.capacity_warned {
                    log::error!(
                        "PassSprites: capacity {} reached; extra sprites are not drawn",
                        MAX_SPRITES
                    );
                    self.capacity_warned = true;
                }
                break;
            }

            // Rotate the sprite-local X/Y axes by the entity transform, with the size and scale
            // folded into their lengths. The camera projection happens in `SpriteDraw::instance`.
            let rotation = transform.rotation_matrix();

            self.sprites.push(SpriteDraw {
                order: sprite.order,
                texture_handle,
                center: [
                    transform.position.x,
                    transform.position.y,
                    transform.position.z,
                ],
                axis_x_world: rotation * Vector3f::new(half_size[0], 0.0, 0.0),
                axis_y_world: rotation * Vector3f::new(0.0, half_size[1], 0.0),
                color: sprite.color,
                uv_flip: [
                    if sprite.flip_x { 1.0 } else { 0.0 },
                    if sprite.flip_y { 1.0 } else { 0.0 },
                ],
                depth_test: if sprite.depth_test { 1.0 } else { 0.0 },
                submerge: [sprite.submerge_height, sprite.submerge_factor],
            });
        }
        if self.sprites.is_empty() {
            return Ok(());
        }
        // Stable sort keeps entity order for sprites sharing the same order value.
        self.sprites.sort_by_key(|sprite| sprite.order);

        let (camera_component, camera_transform) = world.active_camera()?;
        let view_projection = camera_view_projection(camera_component, camera_transform);
        let camera_right = camera_transform.get_right_direction();
        let camera_up = camera_transform.get_up_direction();
        let camera_uniform = SpriteCameraUniform {
            view_projection: view_projection.to_cols_array_2d(),
            camera_right: camera_right.extend(0.0).to_array(),
            camera_up: camera_up.extend(0.0).to_array(),
        };

        let state = self
            .state
            .as_ref()
            .expect("PassSprites: state not initialized — call init() before draw()");

        let queue = renderer.get_queue();
        queue.write_buffer(&state.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));
        for (index, sprite) in self.sprites.iter().enumerate() {
            let instance = sprite.instance(camera_right, camera_up);
            queue.write_buffer(
                &state.instance_buffer,
                (index * std::mem::size_of::<SpriteInstance>()) as u64,
                bytemuck::bytes_of(&instance),
            );
        }

        // One bind group per unique texture (texture + sampler are owned by the texture resource).
        let texture_bind_group_layout = state.pipeline.bind_group_layouts[1].clone();
        self.texture_bind_groups.clear();
        for sprite in &self.sprites {
            if self
                .texture_bind_groups
                .iter()
                .any(|(handle, _)| *handle == sprite.texture_handle)
            {
                continue;
            }
            let Ok(texture) = world.renderer_texture(&sprite.texture_handle) else {
                if !self.missing_texture_warned {
                    log::warn!("PassSprites: sprite texture is missing; the sprite is not drawn");
                    self.missing_texture_warned = true;
                }
                continue;
            };
            let bind_group = renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("pass_sprites_texture_bind_group"),
                    layout: &texture_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&texture.texture_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&texture.sampler),
                        },
                    ],
                });
            self.texture_bind_groups
                .push((sprite.texture_handle, bind_group));
        }

        let color_view = renderer
            .get_render_target_view(self.color_target)
            .expect("PassSprites: color target missing");
        let depth_view = renderer
            .get_render_target_view(self.depth_target)
            .expect("PassSprites: depth target missing");

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pass_sprites_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_vertex_buffer(0, state.instance_buffer.slice(..));
        render_pass.set_bind_group(0, &state.camera_bind_group, &[]);

        // Sprites with a waterline use the submerged shader variant; alternate as needed so the
        // order-based draw order is preserved.
        let mut bound_submerged: Option<bool> = None;
        let mut bound_texture: Option<usize> = None;
        for (index, sprite) in self.sprites.iter().enumerate() {
            let Some(bind_group_index) = self
                .texture_bind_groups
                .iter()
                .position(|(handle, _)| *handle == sprite.texture_handle)
            else {
                continue;
            };
            let submerged = sprite.submerge[1] < 1.0;
            if bound_submerged != Some(submerged) {
                let pipeline = if submerged {
                    &state.pipeline_submerged.pipeline
                } else {
                    &state.pipeline.pipeline
                };
                render_pass.set_pipeline(pipeline);
                bound_submerged = Some(submerged);
            }
            if bound_texture != Some(bind_group_index) {
                render_pass.set_bind_group(1, &self.texture_bind_groups[bind_group_index].1, &[]);
                bound_texture = Some(bind_group_index);
            }
            let instance = index as u32;
            render_pass.draw(0..6, instance..instance + 1);
        }

        Ok(())
    }
}
