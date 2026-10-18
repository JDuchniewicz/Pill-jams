use crate::background_pass::{BackgroundQuadPass, SkyParams};
use crate::cartoon_pass::{CartoonComponent, CartoonPass};
use crate::player::PlayerPhysicsComponent;
use crate::post_process::{DitherPattern, PostProcessParams, PostProcessPass};
use crate::water::WaterParamsComponent;
use crate::water_mesh::WaterLookParams;
use pill_engine::{define_global_component, game::*};

use std::sync::{Arc, Mutex};

// Orthographic size shared with the egui overlay (updated by `camera_zoom_system`).
// `Arc<Mutex<_>>` allows the UI callback (which only receives `&egui::Context`) to read it.
define_global_component!(GameUIStateComponent {
    ortho_size: Arc<Mutex<f32>>,
    draw_sprite_bounds: Arc<Mutex<bool>>,
    draw_background: Arc<Mutex<bool>>,
    // Draw the water mesh with its second UV set (velocity) visualised instead of the normal
    // shading. Read by `WaterMeshPass`.
    show_uv2: Arc<Mutex<bool>>,
    sky: Arc<Mutex<SkyParams>>,
    water_look: Arc<Mutex<WaterLookParams>>,
    post_process: Arc<Mutex<PostProcessParams>>,
});

pub struct Game {}

impl PillGame for Game {
    fn start(&self, engine: &mut Engine) -> Result<()> {
        // --- Basic setup ---

        // Create scene
        let active_scene = engine.create_scene("default")?;
        engine.set_active_scene(active_scene)?;

        // Register graphics components
        engine.register_component::<TransformComponent>(active_scene)?;
        engine.register_component::<MeshRenderingComponent>(active_scene)?;
        engine.register_component::<SpriteRenderingComponent>(active_scene)?;
        engine.register_component::<CartoonComponent>(active_scene)?;
        engine.register_component::<CameraComponent>(active_scene)?;

        // Register game-specific components
        engine.register_component::<AudioListenerComponent>(active_scene)?;
        engine.register_component::<AudioSourceComponent>(active_scene)?;

        // --- Water simulation (must exist before visuals builds the water mesh) ---
        crate::water::start(engine)?;

        // --- Visuals setup ---
        crate::visuals::start(engine, active_scene)?;
        let players = crate::player::start(engine, active_scene).unwrap();
        crate::sounds::start(engine, active_scene, players)?;

        // Add render passes
        engine.add_render_pass(Box::new(CartoonPass::new()))?;
        // 2D sky & water gradient: a background pass, runs before the opaque PBR pass
        engine.add_background_pass(Box::new(BackgroundQuadPass::new()))?;
        // Contrast/gamma, chromatic aberration and vignette over the whole composed frame
        engine.add_post_process_pass(Box::new(PostProcessPass::new()))?;
        // --- Camera scale UI ---

        let ortho_size = Arc::new(Mutex::new(200.0_f32));
        let draw_sprite_bounds = Arc::new(Mutex::new(false));
        let draw_background = Arc::new(Mutex::new(true));
        let show_uv2 = Arc::new(Mutex::new(false));
        let sky = Arc::new(Mutex::new(SkyParams::default()));
        let water_look = Arc::new(Mutex::new(WaterLookParams::default()));
        let post_process = Arc::new(Mutex::new(PostProcessParams::default()));
        // Shared with `player::player_movement_system`; created in `player::start`.
        let player_physics = engine
            .get_global_component::<PlayerPhysicsComponent>()?
            .params
            .clone();
        // Live emission-point height (relative to the water surface) per player, shown in the
        // particle surface UI section.
        let player_emission_height = engine
            .get_global_component::<PlayerPhysicsComponent>()?
            .emission_height
            .clone();
        // Shared with the water simulation systems; created in `water::start`.
        let water_params = engine
            .get_global_component::<WaterParamsComponent>()?
            .params
            .clone();
        engine.add_global_component(GameUIStateComponent {
            ortho_size: Arc::clone(&ortho_size),
            draw_sprite_bounds: Arc::clone(&draw_sprite_bounds),
            draw_background: Arc::clone(&draw_background),
            show_uv2: Arc::clone(&show_uv2),
            sky: Arc::clone(&sky),
            water_look: Arc::clone(&water_look),
            post_process: Arc::clone(&post_process),
        })?;
        engine.set_game_overlay(move |ctx| {
            let ortho_size = *ortho_size
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut draw_sprite_bounds = draw_sprite_bounds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut draw_background = draw_background
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut show_uv2 = show_uv2
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut sky = sky.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut water_look = water_look
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut post_process = post_process
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            egui::Window::new("Wavey")
                .resizable(true)
                .default_width(260.0)
                .default_pos(egui::pos2(10.0, 160.0))
                .show(ctx, |ui| {
                    ui.label(format!("Ortho size: {:.1}", ortho_size));
                    ui.checkbox(&mut draw_sprite_bounds, "Sprite bounds");
                    ui.checkbox(&mut draw_background, "Sky & water background");
                    ui.checkbox(&mut show_uv2, "Water mesh: UV2 (velocity)");

                    ui.separator();
                    egui::CollapsingHeader::new("Sky & water")
                        .default_open(true)
                        .show(ui, |ui| {
                            color_row(ui, "Sky top", &mut sky.sky_top);
                            color_row(ui, "Sky horizon", &mut sky.sky_horizon);
                            color_row(ui, "Water horizon", &mut sky.water_horizon);
                            color_row(ui, "Water bottom", &mut sky.water_bottom);

                            ui.add(
                                egui::Slider::new(&mut sky.horizon, 0.0..=1.0).text("Water line"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.horizon_softness, 0.0..=0.2)
                                    .text("Line softness"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.wave_amplitude, 0.0..=0.05)
                                    .text("Wave amplitude"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.wave_frequency, 0.0..=100.0)
                                    .text("Wave frequency"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.wave_speed, 0.0..=20.0)
                                    .text("Wave speed"),
                            );
                        });

                    egui::CollapsingHeader::new("Sun")
                        .default_open(false)
                        .show(ui, |ui| {
                            color_row(ui, "Glow color", &mut sky.sun_color);
                            ui.add(
                                egui::Slider::new(&mut sky.sun_intensity, 0.0..=2.0)
                                    .text("Intensity"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_position[0], 0.0..=1.0)
                                    .text("Sun X"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_position[1], 0.0..=1.0)
                                    .text("Sun Y"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_radius, 0.001..=1.0).text("Radius"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_softness, 0.0..=1.0)
                                    .text("Softness"),
                            );
                        });

                    egui::CollapsingHeader::new("Water look")
                        .default_open(false)
                        .show(ui, |ui| {
                            color_row(ui, "Water top", &mut water_look.top_color);
                            color_row(ui, "Water bottom", &mut water_look.bottom_color);
                            color_row(ui, "Overlay top", &mut water_look.highlight_top);
                            color_row(ui, "Overlay bottom", &mut water_look.highlight_bottom);
                            ui.add(
                                egui::Slider::new(
                                    &mut water_look.water_gradient_offset,
                                    -100.0..=100.0,
                                )
                                .text("Gradient position"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut water_look.water_gradient_contrast,
                                    0.1..=10.0,
                                )
                                .logarithmic(true)
                                .text("Gradient contrast"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_look.highlight_threshold, 0.0..=1.0)
                                    .text("Overlay threshold"),
                            );

                            color_row(ui, "Foam color", &mut water_look.foam_color);
                            ui.add(
                                egui::Slider::new(&mut water_look.foam_depth, 0.0..=1.0)
                                    .text("Foam depth"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_look.foam_intensity, 0.0..=1.0)
                                    .step_by(0.02)
                                    .text("Foam intensity ×UV2"),
                            );
                        });

                    egui::CollapsingHeader::new("God rays")
                        .default_open(true)
                        .show(ui, |ui| {
                            color_row(ui, "Ray color", &mut water_look.god_rays_color);
                            ui.add(
                                egui::Slider::new(&mut water_look.god_rays_intensity, 0.0..=2.0)
                                    .step_by(0.01)
                                    .text("Intensity"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_look.god_rays_speed, 0.0..=0.2)
                                    .step_by(0.0001)
                                    .text("Travel speed"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_look.god_rays_width, 0.01..=0.4)
                                    .step_by(0.005)
                                    .text("Ray width"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_look.god_rays_angle, -90.0..=90.0)
                                    .text("Angle (deg)"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_look.god_rays_fade, 0.05..=1.0)
                                    .step_by(0.01)
                                    .text("Fade height"),
                            );
                        });
                });

            // --- Player physics: floating panel pinned to the right edge of the window ---
            let mut player_physics = player_physics
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // Initial right-hand column position; the windows stay freely movable/closeable.
            let right_x = (ctx.screen_rect().width() - 300.0).max(10.0);
            egui::Window::new("Player physics")
                .resizable(true)
                .default_width(280.0)
                .default_pos(egui::pos2(right_x, 160.0))
                .show(ctx, |ui| {
                    egui::CollapsingHeader::new("Hull & mass")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut player_physics.hull_half_width, 0.1..=5.0)
                                    .text("Hull half width"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.hull_half_height, 0.05..=2.0)
                                    .text("Hull half height"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.hull_half_depth, 0.05..=2.0)
                                    .text("Hull half depth"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.ship_mass, 0.1..=100.0)
                                    .text("Ship mass"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.friction, 0.0..=2.0)
                                    .text("Friction"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.restitution, 0.0..=1.0)
                                    .text("Restitution"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut player_physics.water_drag_linear,
                                    0.0..=100.0,
                                )
                                .text("Water linear drag"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut player_physics.water_drag_quadratic,
                                    0.0..=100.0,
                                )
                                .text("Water quadratic drag"),
                            );
                        });

                    egui::CollapsingHeader::new("Rigid body")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut player_physics.linear_damping, 0.0..=20.0)
                                    .text("Linear damping"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.angular_damping, 0.0..=20.0)
                                    .text("Angular damping"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.gravity_scale, 0.0..=5.0)
                                    .text("Gravity scale"),
                            );
                            ui.checkbox(&mut player_physics.ccd_enabled, "CCD enabled");
                        });

                    egui::CollapsingHeader::new("Forces")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut player_physics.thrust_force, 0.0..=3000.0)
                                    .text("Thrust force"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.input_deadzone, 0.0..=1.0)
                                    .text("Input dead zone"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut player_physics.buoyancy_strength,
                                    0.0..=1000.0,
                                )
                                .text("Buoyancy strength"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut player_physics.wave_slope_strength,
                                    0.0..=1000.0,
                                )
                                .text("Wave slope force"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.slope_clamp, 0.0..=5.0)
                                    .text("Slope clamp"),
                            );
                        });

                    egui::CollapsingHeader::new("Hull samples")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut player_physics.hull_sample_x, 0.0..=2.0)
                                    .text("Sample X"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.hull_sample_y, 0.0..=2.0)
                                    .text("Sample Y"),
                            );
                            ui.add(
                                egui::Slider::new(&mut player_physics.hull_center_y, 0.0..=2.0)
                                    .text("Centre sample Y"),
                            );
                        });

                    // The emitter always runs; these control the water-surface band that decides
                    // each particle's opacity at spawn time.
                    egui::CollapsingHeader::new("Particle surface")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(
                                    &mut player_physics.particle_surface_center,
                                    -4.0..=1.0,
                                )
                                .step_by(0.01)
                                .text("Emission height"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut player_physics.particle_surface_band,
                                    0.0..=1.5,
                                )
                                .step_by(0.01)
                                .text("Surface band ±"),
                            );
                            let height = player_emission_height
                                .lock()
                                .map(|heights| heights[0])
                                .unwrap_or(f32::NAN);
                            let center = player_physics.particle_surface_center;
                            let band = player_physics.particle_surface_band.max(0.0);
                            let on_surface = (height - center).abs() <= band;
                            ui.label(format!(
                                "P0 emission height: {height:.2} | band {:.2}..{:.2} ({})",
                                center - band,
                                center + band,
                                if on_surface { "visible" } else { "transparent" }
                            ));
                        });
                });

            // --- Water simulation parameters ---
            let mut water_params = water_params
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            egui::Window::new("Water simulation")
                .resizable(true)
                .default_width(280.0)
                .default_pos(egui::pos2(right_x, 520.0))
                .show(ctx, |ui| {
                    egui::CollapsingHeader::new("Wave equation")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut water_params.wave_speed, 0.05..=5.0)
                                    .text("Wave speed"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_params.damping, 0.0..=2.0)
                                    .text("Damping"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_params.restoring_force, 0.0..=5.0)
                                    .text("Restoring force"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut water_params.baseline_restore_rate,
                                    0.0..=10.0,
                                )
                                .text("Baseline restore rate"),
                            );
                        });

                    egui::CollapsingHeader::new("Kicks & bombs")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut water_params.kick_strength, 0.0..=500.0)
                                    .text("Kick strength"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_params.influence_radius, 0.01..=0.5)
                                    .text("Influence radius"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_params.time_to_detonate, 0.0..=5.0)
                                    .text("Bomb fuse (s)"),
                            );
                        });

                    egui::CollapsingHeader::new("Swells")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut water_params.swell_interval_min, 0.1..=10.0)
                                    .text("Interval min (s)"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_params.swell_interval_max, 0.1..=10.0)
                                    .text("Interval max (s)"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut water_params.swell_strength_min,
                                    0.0..=100.0,
                                )
                                .text("Strength min"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut water_params.swell_strength_max,
                                    0.0..=100.0,
                                )
                                .text("Strength max"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut water_params.swell_direction_spread,
                                    0.0..=1.0,
                                )
                                .text("Direction spread"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    &mut water_params.swell_direction_jitter,
                                    0.0..=1.0,
                                )
                                .text("Direction jitter"),
                            );
                        });

                    egui::CollapsingHeader::new("Edge waves")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut water_params.wave_pool_interval, 0.1..=10.0)
                                    .text("Interval (s)"),
                            );
                        });

                    egui::CollapsingHeader::new("Surface query")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(
                                    &mut water_params.height_sample_window,
                                    0.0..=0.5,
                                )
                                .text("Sample window"),
                            );
                            ui.add(
                                egui::Slider::new(&mut water_params.height_sample_limit, 1..=128)
                                    .text("Sample limit"),
                            );
                        });
                });

            // --- Post-processing: one pass, three shader sections ---
            egui::Window::new("Post processing")
                .resizable(true)
                .default_width(280.0)
                .default_pos(egui::pos2(
                    10.0,
                    (ctx.screen_rect().height() - 420.0).max(10.0),
                ))
                .show(ctx, |ui| {
                    egui::CollapsingHeader::new("1. Contrast & gamma")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut post_process.contrast, 0.0..=3.0)
                                    .text("Contrast"),
                            );
                            ui.add(
                                egui::Slider::new(&mut post_process.gamma, 0.2..=3.0).text("Gamma"),
                            );
                        });

                    egui::CollapsingHeader::new("2. Chromatic aberration")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut post_process.aberration_strength, 0.0..=0.1)
                                    .text("Strength"),
                            );
                        });

                    egui::CollapsingHeader::new("3. Vignette")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Slider::new(&mut post_process.vignette_strength, 0.0..=1.0)
                                    .text("Strength"),
                            );
                            ui.add(
                                egui::Slider::new(&mut post_process.vignette_radius, 0.0..=1.0)
                                    .text("Radius"),
                            );
                            ui.add(
                                egui::Slider::new(&mut post_process.vignette_softness, 0.001..=1.0)
                                    .text("Softness"),
                            );
                        });

                    egui::CollapsingHeader::new("4. Dithering")
                        .default_open(true)
                        .show(ui, |ui| {
                            egui::ComboBox::from_label("Pattern")
                                .selected_text(post_process.dither_pattern.label())
                                .show_ui(ui, |ui| {
                                    for pattern in DitherPattern::ALL {
                                        ui.selectable_value(
                                            &mut post_process.dither_pattern,
                                            pattern,
                                            pattern.label(),
                                        );
                                    }
                                });
                            ui.add(
                                egui::Slider::new(&mut post_process.dither_strength, 0.0..=1.0)
                                    .step_by(0.02)
                                    .text("Strength"),
                            );
                            ui.add(
                                egui::Slider::new(&mut post_process.dither_levels, 2..=256)
                                    .logarithmic(true)
                                    .text("Levels"),
                            );
                            ui.add(
                                egui::Slider::new(&mut post_process.dither_scale, 1.0..=8.0)
                                    .step_by(0.25)
                                    .text("Pattern scale (px)"),
                            );

                            // Pattern-specific controls, shown only for the patterns that use them.
                            if post_process.dither_pattern.uses_extra_parameters() {
                                ui.separator();
                                ui.add(
                                    egui::Slider::new(
                                        &mut post_process.dither_angle,
                                        -180.0..=180.0,
                                    )
                                    .text("Angle (deg)"),
                                );
                                let width_label = match post_process.dither_pattern {
                                    DitherPattern::Halftone => "Dot size",
                                    _ => "Line width",
                                };
                                ui.add(
                                    egui::Slider::new(&mut post_process.dither_width, 0.01..=1.0)
                                        .step_by(0.01)
                                        .text(width_label),
                                );
                                ui.add(
                                    egui::Slider::new(&mut post_process.dither_softness, 0.0..=1.0)
                                        .text("Softness"),
                                );
                            }
                        });
                });
        })?;

        Ok(())
    }
}

/// One labelled colour picker row.
fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        ui.color_edit_button_rgb(color);
        ui.label(label);
    });
}
