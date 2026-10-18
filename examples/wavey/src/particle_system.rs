use pill_engine::{define_component, game::*};

const VERTEX_SHADER: &str = include_str!("../res/shaders/particle_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/particle_fragment.wgsl");

/// Hard cap for one emitter. The default emitter (10/s, 1 s lifetime) needs at most 10 slots;
/// the extra headroom keeps the Vec preallocated if the rate is raised at runtime.
const MAX_PARTICLES_PER_SYSTEM: usize = 128;
/// Hard cap for the shared GPU instance buffer (all emitters combined).
const MAX_PARTICLES: usize = 1024;

/// Opacity of a particle emitted from inside the water-surface band.
const SURFACE_OPACITY: f32 = 0.8;

const BIND_GROUP_FRAME: usize = 0;
const BIND_GROUP_TEXTURE: usize = 1;

/// One simulated particle. Positions/velocities are in world units, time in seconds.
#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub position: Vector3f,
    pub velocity: Vector3f,
    /// Non-gravity acceleration applied to this particle; fades linearly over its lifetime.
    pub force: Vector3f,
    pub age: f32,
    /// Opacity chosen at spawn from the emission height relative to the water surface.
    pub alpha: f32,
}

// CPU-simulated particle emitter. Any entity with this component emits particles at its
// `TransformComponent` position; `ParticlePass` renders all particles with one instanced draw.
define_component!(ParticleSystemComponent {
    // Texture sampled by every particle quad.
    texture: TextureHandle,
    // Particles spawned per second.
    rate_per_second: f32,
    // Seconds a particle stays alive.
    lifetime: f32,
    // Initial non-gravity acceleration, in world units / s².
    initial_force: Vector3f,
    // Downward acceleration (positive = down), in world units / s².
    gravity: f32,
    // Amount of non-gravity acceleration lost per `attenuation_interval` seconds.
    force_attenuation: f32,
    // Seconds between `force_attenuation` steps.
    attenuation_interval: f32,
    // Quad half-size in world units.
    particle_size: f32,
    // Linear RGBA tint; alpha is multiplied by the lifetime fade in `ParticlePass`.
    color: [f32; 4],
    // Random spawn-position jitter (world units, XY) so particles do not perfectly overlap.
    spawn_spread: f32,
    // When false, no new particles are emitted (existing ones still update).
    enabled: bool,
    // Set by the player: true while thrust input is actually being applied. Only then do new
    // particles spawn; the water surface still decides their opacity.
    emitting: bool,
    // Water surface height under the emitter, refreshed every frame by the player system.
    water_level: f32,
    // Emission height (relative to the water) that counts as being on the surface.
    surface_center: f32,
    // Half-width of the surface band: inside it particles are drawn at SURFACE_OPACITY,
    // outside it they are fully transparent.
    surface_band: f32,
    // X offset of the spawn point from the entity position (mirrored by the player's flip).
    spawn_offset_x: f32,
    // Alive particles, preallocated by `new` to avoid per-spawn allocations.
    particles: Vec<Particle>,
    // Fractional emission carry between frames.
    spawn_accumulator: f32,
});

impl ParticleSystemComponent {
    /// Foam puff emitter: 10 particles/s, 1 s lifetime, initial rightward force, gravity and
    /// linear force attenuation (1 unit per 0.3 s) as requested.
    pub fn new(texture: TextureHandle) -> Self {
        Self {
            texture,
            rate_per_second: 10.0,
            lifetime: 0.8,
            initial_force: Vector3f::new(15.0, 0.0, 0.0),
            gravity: 5.0,
            force_attenuation: 1.0,
            attenuation_interval: 0.4,
            particle_size: 0.9,
            color: [1.0, 1.0, 1.0, 1.0],
            spawn_spread: 0.6,
            enabled: true,
            water_level: 0.0,
            surface_center: -1.45,
            surface_band: 0.2,
            emitting: false,
            spawn_offset_x: 0.0,
            particles: Vec::with_capacity(MAX_PARTICLES_PER_SYSTEM),
            spawn_accumulator: 0.0,
        }
    }

    /// Refreshes this frame's player state.
    ///
    /// Particles spawn only while `thrusting` is true; the water surface then decides each
    /// particle's opacity at spawn time. A particle is drawn when the emission point
    /// (`entity position + spawn_offset_x`) sits within `surface_band` world units of
    /// `surface_center` (measured relative to the water level), otherwise it is fully transparent.
    ///
    /// `thrust_input` is the raw input axis; the ship's applied thrust is `-thrust_input`, so new
    /// particles are pushed in the opposite direction (exhaust behind the ship). `flip` mirrors
    /// the spawn point on X (`8 * flip`).
    pub fn update_from_player(
        &mut self,
        thrusting: bool,
        thrust_input: f32,
        flip: i32,
        water_level: f32,
        surface_center: f32,
        surface_band: f32,
    ) {
        self.emitting = thrusting;
        self.spawn_offset_x = 8.0 * flip as f32;
        self.water_level = water_level;
        self.surface_center = surface_center;
        self.surface_band = surface_band.max(0.0);
        if thrusting {
            self.initial_force = Vector3f::new(thrust_input.signum() * 15.0, 0.0, 0.0);
        }
    }
}

// --- Camera / instance data shared with the shaders ---

/// Mirrors `Camera` in particle_vertex.hlsl.
#[repr(C)]
#[derive(Copy, Clone)]
struct ParticleCameraUniform {
    view_projection: [[f32; 4]; 4],
    camera_right: [f32; 4],
    camera_up: [f32; 4],
}

// The engine re-exports `bytemuck`; the marker traits are implemented manually so this game
// crate does not need a direct dependency just for the derive macros.
unsafe impl pill_engine::game::bytemuck::Zeroable for ParticleCameraUniform {}
unsafe impl pill_engine::game::bytemuck::Pod for ParticleCameraUniform {}

/// One particle instance: quad centre, half-size and linear RGBA tint.
#[repr(C)]
#[derive(Copy, Clone)]
struct ParticleInstance {
    center: [f32; 3], // offset 0
    size: f32,        // offset 12
    color: [f32; 4],  // offset 16
}

unsafe impl pill_engine::game::bytemuck::Zeroable for ParticleInstance {}
unsafe impl pill_engine::game::bytemuck::Pod for ParticleInstance {}

const _: () = assert!(std::mem::size_of::<ParticleInstance>() == 32);

impl ParticleInstance {
    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ParticleInstance>() as wgpu::BufferAddress,
            // One instance per particle; the six quad vertices come from SV_VertexID.
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
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

// --- Pass ---

struct ParticlePassState {
    pipeline: Pipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    instance_buffer: wgpu::Buffer,
}

/// Game-defined scene pass: draws every emitter's particles as instanced camera-facing quads.
///
/// All particles are packed into one buffer and drawn with one instanced call per unique texture,
/// so the cost is one upload and (for the usual single texture) one draw call per frame.
pub struct ParticlePass {
    state: Option<ParticlePassState>,
    instances: Vec<ParticleInstance>,
    /// Contiguous instance ranges sharing a texture: (texture, start, end).
    draw_ranges: Vec<(TextureHandle, u32, u32)>,
    texture_bind_groups: Vec<(TextureHandle, wgpu::BindGroup)>,
}

impl ParticlePass {
    pub fn new() -> Self {
        Self {
            state: None,
            instances: Vec::with_capacity(MAX_PARTICLES),
            draw_ranges: Vec::with_capacity(8),
            texture_bind_groups: Vec::new(),
        }
    }
}

impl Default for ParticlePass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for ParticlePass {
    fn get_label(&self) -> &str {
        "particle_pass"
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let bind_groups = vec![
            // 0: camera uniform
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
            // 1: particle texture + sampler
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

        let pipeline = renderer.create_pipeline(PipelineDesc {
            label: Some("particle_pass"),
            vs: ShaderDesc {
                source: VERTEX_SHADER,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: FRAGMENT_SHADER,
                entry_func: "fs_main",
            },
            vertex_buffers: &[ParticleInstance::layout()],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_TARGET_FORMAT,
                // Particles blend over each other and never occlude scene geometry.
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
        })?;

        let camera_buffer = renderer.create_buffer(BufferDesc {
            label: Some("particle_pass_camera"),
            byte_size: std::mem::size_of::<ParticleCameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;

        let camera_bind_group =
            renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("particle_pass_camera_bind_group"),
                    layout: &pipeline.bind_group_layouts[BIND_GROUP_FRAME],
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: camera_buffer.as_entire_binding(),
                    }],
                });

        let instance_buffer = renderer.create_buffer(BufferDesc {
            label: Some("particle_pass_instances"),
            byte_size: (MAX_PARTICLES * std::mem::size_of::<ParticleInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        })?;

        self.state = Some(ParticlePassState {
            pipeline,
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
        let Ok(systems) = world.query::<ParticleSystemComponent>() else {
            return Ok(());
        };
        let Some(state) = self.state.as_ref() else {
            return Ok(());
        };

        // Pack all emitters into one instance buffer. Ranges with the same texture are merged so
        // the common case (one foam texture) is a single instanced draw.
        self.instances.clear();
        self.draw_ranges.clear();
        for system in systems.data.iter().flatten() {
            if system.particles.is_empty() || self.instances.len() >= MAX_PARTICLES {
                continue;
            }
            let start = self.instances.len() as u32;
            let lifetime = system.lifetime.max(0.0001);
            for particle in &system.particles {
                if self.instances.len() >= MAX_PARTICLES {
                    break;
                }
                let fade = (1.0 - particle.age / lifetime).clamp(0.0, 1.0);
                self.instances.push(ParticleInstance {
                    center: particle.position.to_array(),
                    size: system.particle_size,
                    color: [
                        system.color[0],
                        system.color[1],
                        system.color[2],
                        system.color[3] * fade * particle.alpha,
                    ],
                });
            }
            let end = self.instances.len() as u32;
            if end == start {
                continue;
            }
            if let Some(last) = self.draw_ranges.last_mut() {
                if last.0 == system.texture {
                    last.2 = end;
                    continue;
                }
            }
            self.draw_ranges.push((system.texture, start, end));
        }
        if self.instances.is_empty() {
            return Ok(());
        }

        let (camera, camera_transform) = world.active_camera()?;
        let camera_uniform = ParticleCameraUniform {
            view_projection: camera_view_projection(camera, camera_transform).to_cols_array_2d(),
            camera_right: camera_transform
                .get_right_direction()
                .extend(0.0)
                .to_array(),
            camera_up: camera_transform.get_up_direction().extend(0.0).to_array(),
        };
        let queue = renderer.get_queue();
        queue.write_buffer(&state.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));
        queue.write_buffer(
            &state.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );

        // Build texture bind groups on first use (one per unique texture).
        for (texture_handle, _, _) in &self.draw_ranges {
            let cached = self
                .texture_bind_groups
                .iter()
                .any(|(handle, _)| handle == texture_handle);
            if !cached {
                let texture = world.renderer_texture(texture_handle)?;
                let bind_group =
                    renderer
                        .get_device()
                        .create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("particle_pass_texture_bind_group"),
                            layout: &state.pipeline.bind_group_layouts[BIND_GROUP_TEXTURE],
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(
                                        &texture.texture_view,
                                    ),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(&texture.sampler),
                                },
                            ],
                        });
                self.texture_bind_groups.push((*texture_handle, bind_group));
            }
        }

        // Composite into the HDR scene, loading the existing opaque results.
        let render_state = world.get_global::<RenderStateComponent>()?;
        let color_view = renderer
            .get_render_target_view(render_state.hdr_target())
            .expect("ParticlePass: HDR target missing");
        let depth_view = renderer
            .get_render_target_view(render_state.depth_target())
            .expect("ParticlePass: depth target missing");
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("particle_pass_render_pass"),
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
        render_pass.set_pipeline(&state.pipeline.pipeline);
        render_pass.set_bind_group(BIND_GROUP_FRAME as u32, &state.camera_bind_group, &[]);
        render_pass.set_vertex_buffer(0, state.instance_buffer.slice(..));

        for (texture_handle, start, end) in &self.draw_ranges {
            let (_, bind_group) = self
                .texture_bind_groups
                .iter()
                .find(|(handle, _)| handle == texture_handle)
                .expect("ParticlePass: texture bind group built above");
            render_pass.set_bind_group(BIND_GROUP_TEXTURE as u32, bind_group, &[]);
            render_pass.draw(0..6, *start..*end);
        }

        Ok(())
    }
}

// --- Simulation ---

fn particle_system_update(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    if delta_time <= 0.0 {
        return Ok(());
    }

    for (_, system, transform) in
        engine.iterate_two_components_mut::<ParticleSystemComponent, TransformComponent>()?
    {
        // Only emit while the player applies thrust; the surface band sets the opacity below.
        let can_emit = system.enabled
            && system.emitting
            && system.rate_per_second > 0.0
            && system.lifetime > 0.0;
        if can_emit {
            let source = transform.position + Vector3f::new(system.spawn_offset_x, 0.0, 0.0);
            // Fixed-rate emission with a carry accumulator: no allocations after the Vec is warm.
            system.spawn_accumulator += system.rate_per_second * delta_time;
            while system.spawn_accumulator >= 1.0 {
                system.spawn_accumulator -= 1.0;
                if system.particles.len() >= MAX_PARTICLES_PER_SYSTEM {
                    break;
                }
                let jitter = if system.spawn_spread > 0.0 {
                    Vector3f::new(
                        rand::random_range(-system.spawn_spread..system.spawn_spread),
                        rand::random_range(-system.spawn_spread..system.spawn_spread),
                        0.0,
                    )
                } else {
                    Vector3f::ZERO
                };
                // Opacity from the emission point's height relative to the water surface:
                // inside the surface band -> 0.8, anywhere else -> 0.0.
                let emission_height = (source.y - system.water_level) - system.surface_center;
                let alpha = if emission_height.abs() <= system.surface_band {
                    SURFACE_OPACITY
                } else {
                    0.0
                };
                system.particles.push(Particle {
                    position: source + jitter,
                    velocity: Vector3f::ZERO,
                    force: system.initial_force,
                    age: 0.0,
                    alpha,
                });
            }
            if system.particles.len() >= MAX_PARTICLES_PER_SYSTEM {
                // Do not build up a burst while the emitter is saturated.
                system.spawn_accumulator = system.spawn_accumulator.min(0.999);
            }
        } else {
            // Dropping thrust must not leave a fractional charge that pops later.
            system.spawn_accumulator = 0.0;
        }

        // Non-gravity force fades linearly: `force_attenuation` units per interval.
        let decay_per_second = if system.attenuation_interval > 0.0 {
            system.force_attenuation / system.attenuation_interval
        } else {
            0.0
        };
        let gravity = Vector3f::new(0.0, -system.gravity, 0.0);
        for particle in &mut system.particles {
            particle.age += delta_time;
            let force_magnitude = particle.force.length();
            if force_magnitude > 0.0 && decay_per_second > 0.0 {
                let remaining = (force_magnitude - decay_per_second * delta_time).max(0.0);
                particle.force *= remaining / force_magnitude;
            }
            particle.velocity += (particle.force + gravity) * delta_time;
            particle.position += particle.velocity * delta_time;
        }
        system
            .particles
            .retain(|particle| particle.age < system.lifetime);
    }

    Ok(())
}

/// Registers the component, system and render pass. Call before building emitter entities.
pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    engine.register_component::<ParticleSystemComponent>(active_scene)?;
    engine.add_render_pass(Box::new(ParticlePass::new()))?;
    engine.add_system("particle_system_update", particle_system_update)?;
    Ok(())
}
