use pill_engine::{define_global_component, game::*};
use std::sync::{Arc, Mutex};

use crate::game::GameUIStateComponent;
use crate::post_process::PostProcessParams;

/// Minimum time between two queued requests of the same event type.
const EVENT_COOLDOWN: f32 = 2.0;
/// Number of event kinds, i.e. the size of the per-kind cooldown table.
const EVENT_KIND_COUNT: usize = 5;

// --- Tunables (edited in the egui overlay, snapshotted when a request is created) ---

/// Every parameter the effect events use. The collision system copies these values into a request
/// when it is queued, so slider changes only affect future events.
#[derive(Clone, Debug)]
pub struct EffectParams {
    // --- Screen shake ---
    pub shake_strength: f32,
    pub shake_duration: f32,
    pub shake_damping: f32,
    pub shake_frequency: f32,
    // --- Vignette ---
    pub vignette_peak: f32,
    pub vignette_rise: f32,
    pub vignette_hold: f32,
    pub vignette_fall: f32,
    // --- Chromatic aberration ---
    pub aberration_peak: f32,
    pub aberration_rise: f32,
    pub aberration_hold: f32,
    pub aberration_fall: f32,
    // --- Time slowdown ---
    pub slowmo_scale: f32,
    pub slowmo_rise: f32,
    pub slowmo_hold: f32,
    pub slowmo_fall: f32,
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            shake_strength: 1.5,
            shake_duration: 0.6,
            shake_damping: 6.0,
            shake_frequency: 45.0,
            vignette_peak: 1.0,
            vignette_rise: 0.3,
            vignette_hold: 0.2,
            vignette_fall: 1.0,
            aberration_peak: 0.03,
            aberration_rise: 0.3,
            aberration_hold: 0.2,
            aberration_fall: 1.0,
            slowmo_scale: 0.35,
            slowmo_rise: 0.3,
            slowmo_hold: 0.2,
            slowmo_fall: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectKind {
    ScreenShake,
    Vignette,
    ChromaticAberration,
    SlowMotion,
    Bomb,
}

impl EffectKind {
    pub const ALL: [EffectKind; EVENT_KIND_COUNT] = [
        EffectKind::ScreenShake,
        EffectKind::Vignette,
        EffectKind::ChromaticAberration,
        EffectKind::SlowMotion,
        EffectKind::Bomb,
    ];

    pub fn label(self) -> &'static str {
        match self {
            EffectKind::ScreenShake => "Screenshake",
            EffectKind::Vignette => "Vignette",
            EffectKind::ChromaticAberration => "Chromatic aberration",
            EffectKind::SlowMotion => "Time slowdown",
            EffectKind::Bomb => "Surfacing bomb",
        }
    }
}

// --- Event requests ---

/// Exponential ramp helper: 0 -> 1 over `rise`, hold at 1 for `hold`, then 1 -> 0 over `fall`.
fn envelope(age: f32, rise: f32, hold: f32, fall: f32) -> f32 {
    const SHARPNESS: f32 = 5.0;
    let floor = (-SHARPNESS).exp();
    let rise = rise.max(0.0001);
    let fall = fall.max(0.0001);

    if age < rise {
        ((1.0 - (-SHARPNESS * (age / rise)).exp()) / (1.0 - floor)).clamp(0.0, 1.0)
    } else if age < rise + hold {
        1.0
    } else {
        let t = ((age - rise - hold) / fall).clamp(0.0, 1.0);
        (((-SHARPNESS * t).exp() - floor) / (1.0 - floor)).clamp(0.0, 1.0)
    }
}

/// Camera shake that fades out: the offset is a sum of two sines with an exponentially decaying
/// amplitude.
#[derive(Clone, Debug)]
pub struct ScreenShakeEvent {
    pub age: f32,
    pub duration: f32,
    pub strength: f32,
    pub damping: f32,
    pub frequency: f32,
    phase: f32,
}

impl ScreenShakeEvent {
    pub fn new(params: &EffectParams) -> Self {
        Self {
            age: 0.0,
            duration: params.shake_duration.max(0.0),
            strength: params.shake_strength,
            damping: params.shake_damping,
            frequency: params.shake_frequency,
            phase: rand::random_range(0.0..std::f32::consts::TAU),
        }
    }

    fn is_finished(&self) -> bool {
        self.age >= self.duration
    }

    /// Adds this frame's camera offset (screen space: x = right, y = up).
    fn handler(&mut self, delta_time: f32, frame: &mut FrameEffects) {
        self.age += delta_time;
        let amplitude = self.strength * (-self.damping.max(0.0) * self.age).exp();
        let t = self.age * self.frequency + self.phase;
        frame.shake += Vector2f::new(t.sin(), (t * 1.618_034).cos()) * amplitude;
    }
}

/// Drives a post-process value: exponential rise, short hold, exponential fall back to `base`.
#[derive(Clone, Debug)]
pub struct VignetteEvent {
    pub age: f32,
    pub base: f32,
    pub peak: f32,
    pub rise: f32,
    pub hold: f32,
    pub fall: f32,
}

impl VignetteEvent {
    pub fn new(params: &EffectParams, base: f32) -> Self {
        Self {
            age: 0.0,
            base,
            peak: params.vignette_peak,
            rise: params.vignette_rise,
            hold: params.vignette_hold,
            fall: params.vignette_fall,
        }
    }

    fn lifetime(&self) -> f32 {
        (self.rise + self.hold + self.fall).max(0.0001)
    }

    fn is_finished(&self) -> bool {
        self.age >= self.lifetime()
    }

    fn handler(&mut self, engine: &mut Engine, delta_time: f32) -> Result<()> {
        self.age += delta_time;
        let value = self.base
            + (self.peak - self.base) * envelope(self.age, self.rise, self.hold, self.fall);
        with_post_process(engine, |post| post.vignette_strength = value)
    }

    fn finish(&self, engine: &mut Engine) -> Result<()> {
        with_post_process(engine, |post| post.vignette_strength = self.base)
    }
}

/// Same shape as the vignette, but drives the chromatic aberration strength.
#[derive(Clone, Debug)]
pub struct ChromaticAberrationEvent {
    pub age: f32,
    pub base: f32,
    pub peak: f32,
    pub rise: f32,
    pub hold: f32,
    pub fall: f32,
}

impl ChromaticAberrationEvent {
    pub fn new(params: &EffectParams, base: f32) -> Self {
        Self {
            age: 0.0,
            base,
            peak: params.aberration_peak,
            rise: params.aberration_rise,
            hold: params.aberration_hold,
            fall: params.aberration_fall,
        }
    }

    fn lifetime(&self) -> f32 {
        (self.rise + self.hold + self.fall).max(0.0001)
    }

    fn is_finished(&self) -> bool {
        self.age >= self.lifetime()
    }

    fn handler(&mut self, engine: &mut Engine, delta_time: f32) -> Result<()> {
        self.age += delta_time;
        let value = self.base
            + (self.peak - self.base) * envelope(self.age, self.rise, self.hold, self.fall);
        with_post_process(engine, |post| post.aberration_strength = value)
    }

    fn finish(&self, engine: &mut Engine) -> Result<()> {
        with_post_process(engine, |post| post.aberration_strength = self.base)
    }
}

/// Same shape, but scales the engine's delta time for the rest of the frame.
#[derive(Clone, Debug)]
pub struct SlowMotionEvent {
    pub age: f32,
    pub target_scale: f32,
    pub rise: f32,
    pub hold: f32,
    pub fall: f32,
}

impl SlowMotionEvent {
    pub fn new(params: &EffectParams) -> Self {
        Self {
            age: 0.0,
            target_scale: params.slowmo_scale,
            rise: params.slowmo_rise,
            hold: params.slowmo_hold,
            fall: params.slowmo_fall,
        }
    }

    fn lifetime(&self) -> f32 {
        (self.rise + self.hold + self.fall).max(0.0001)
    }

    fn is_finished(&self) -> bool {
        self.age >= self.lifetime()
    }

    fn handler(&mut self, delta_time: f32, frame: &mut FrameEffects) {
        self.age += delta_time;
        let amount = envelope(self.age, self.rise, self.hold, self.fall);
        let scale = 1.0 - (1.0 - self.target_scale) * amount;
        frame.time_scale = frame.time_scale.min(scale.clamp(0.01, 1.0));
    }
}

/// Drops a bomb into the water at a world position, scaled by `power`. It only lives for the frame
/// on which it is processed (the splash itself is owned by the water simulation).
#[derive(Clone, Debug)]
pub struct BombEvent {
    pub age: f32,
    pub position_x: f32,
    pub position_y: f32,
    pub power: f32,
    /// Optional fuse override; `Some(0.0)` detonates on this frame.
    pub fuse: Option<f32>,
}

impl BombEvent {
    pub fn new(position_x: f32, position_y: f32, power: f32) -> Self {
        Self {
            age: 0.0,
            position_x,
            position_y,
            power,
            fuse: None,
        }
    }

    /// Creates a bomb that detonates immediately (no fuse).
    pub fn immediate(position_x: f32, position_y: f32, power: f32) -> Self {
        Self {
            fuse: Some(0.0),
            ..Self::new(position_x, position_y, power)
        }
    }

    fn is_finished(&self) -> bool {
        // A one-shot: it has done its job once the handler ran.
        self.age > 0.0
    }

    fn handler(&mut self, engine: &mut Engine, delta_time: f32) -> Result<()> {
        self.age += delta_time.max(f32::EPSILON);
        crate::water::add_bomb_with_power_and_fuse(
            engine,
            self.position_x,
            self.position_y,
            self.power,
            self.fuse,
        )
    }
}

/// One queued request. Every variant carries its own parameters, age and lifetime, and knows how
/// to process itself through `handler`.
#[derive(Clone, Debug)]
pub enum EffectEvent {
    ScreenShake(ScreenShakeEvent),
    Vignette(VignetteEvent),
    ChromaticAberration(ChromaticAberrationEvent),
    SlowMotion(SlowMotionEvent),
    Bomb(BombEvent),
}

impl EffectEvent {
    pub fn kind(&self) -> EffectKind {
        match self {
            EffectEvent::ScreenShake(_) => EffectKind::ScreenShake,
            EffectEvent::Vignette(_) => EffectKind::Vignette,
            EffectEvent::ChromaticAberration(_) => EffectKind::ChromaticAberration,
            EffectEvent::SlowMotion(_) => EffectKind::SlowMotion,
            EffectEvent::Bomb(_) => EffectKind::Bomb,
        }
    }

    /// Per-frame handler: advances the event's age and applies the effect for this frame.
    fn handler(
        &mut self,
        engine: &mut Engine,
        delta_time: f32,
        frame: &mut FrameEffects,
    ) -> Result<()> {
        match self {
            EffectEvent::ScreenShake(event) => {
                event.handler(delta_time, frame);
                Ok(())
            }
            EffectEvent::Vignette(event) => event.handler(engine, delta_time),
            EffectEvent::ChromaticAberration(event) => event.handler(engine, delta_time),
            EffectEvent::SlowMotion(event) => {
                event.handler(delta_time, frame);
                Ok(())
            }
            EffectEvent::Bomb(event) => event.handler(engine, delta_time),
        }
    }

    fn is_finished(&self) -> bool {
        match self {
            EffectEvent::ScreenShake(event) => event.is_finished(),
            EffectEvent::Vignette(event) => event.is_finished(),
            EffectEvent::ChromaticAberration(event) => event.is_finished(),
            EffectEvent::SlowMotion(event) => event.is_finished(),
            EffectEvent::Bomb(event) => event.is_finished(),
        }
    }

    /// Restores the stock values once the event is removed from the in-progress queue.
    fn finish(&self, engine: &mut Engine) -> Result<()> {
        match self {
            EffectEvent::ScreenShake(_) | EffectEvent::SlowMotion(_) | EffectEvent::Bomb(_) => {
                Ok(())
            }
            EffectEvent::Vignette(event) => event.finish(engine),
            EffectEvent::ChromaticAberration(event) => event.finish(engine),
        }
    }
}

/// Per-frame accumulated results of all in-progress events.
#[derive(Clone, Copy, Debug)]
struct FrameEffects {
    /// Camera offset in screen space (x = right, y = up).
    shake: Vector2f,
    /// Multiplier applied to `TimeComponent::delta_time` for the rest of the frame.
    time_scale: f32,
}

impl Default for FrameEffects {
    fn default() -> Self {
        Self {
            shake: Vector2f::ZERO,
            time_scale: 1.0,
        }
    }
}

// --- Global queue + processor system ---

define_global_component!(EffectQueueComponent {
    // Requests waiting to be picked up (one per event type at most, thanks to the cooldown).
    queue: Vec<EffectEvent>,
    // Events that were dequeued and are processed every frame until they finish.
    in_progress: Vec<EffectEvent>,
    // Remaining cooldown per `EffectKind`, in seconds.
    cooldowns: [f32; EVENT_KIND_COUNT],
    // Camera offset applied last frame, so only the difference has to be applied again.
    applied_shake: Vector2f,
    // Live-edited effect parameters (sliders in the "Effects" window).
    params: Arc<Mutex<EffectParams>>,
});

pub fn start(engine: &mut Engine, _active_scene: SceneHandle) -> Result<()> {
    engine.add_global_component(EffectQueueComponent {
        queue: Vec::new(),
        in_progress: Vec::new(),
        cooldowns: [0.0; EVENT_KIND_COUNT],
        applied_shake: Vector2f::ZERO,
        params: Arc::new(Mutex::new(EffectParams::default())),
    })?;
    // Registered first so the scaled delta time is visible to every other game system.
    engine.add_system("effect_event_system", effect_event_system)?;
    Ok(())
}

/// Queues one request. Respects the per-type cooldown (`EVENT_COOLDOWN` per event type).
pub fn push_effect(engine: &mut Engine, event: EffectEvent) -> Result<()> {
    push_effect_with_cooldown(engine, event, EVENT_COOLDOWN)
}

/// Queues one request with an explicit cooldown for that event type.
pub fn push_effect_with_cooldown(
    engine: &mut Engine,
    event: EffectEvent,
    cooldown: f32,
) -> Result<()> {
    let state = engine.get_global_component_mut::<EffectQueueComponent>()?;
    let kind = event.kind() as usize;
    if state.cooldowns[kind] > 0.0 {
        return Ok(());
    }
    state.cooldowns[kind] = cooldown.max(0.0);
    state.queue.push(event);
    Ok(())
}

/// Queues one request of every event type, using the current GUI parameter values. Called when the
/// players collide.
pub fn push_collision_effects(engine: &mut Engine) -> Result<()> {
    let params = {
        let state = engine.get_global_component::<EffectQueueComponent>()?;
        let guard = state
            .params
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    };
    let (vignette_base, aberration_base) = current_post_process(engine)?;

    push_effect(
        engine,
        EffectEvent::ScreenShake(ScreenShakeEvent::new(&params)),
    )?;
    push_effect(
        engine,
        EffectEvent::Vignette(VignetteEvent::new(&params, vignette_base)),
    )?;
    push_effect(
        engine,
        EffectEvent::ChromaticAberration(ChromaticAberrationEvent::new(&params, aberration_base)),
    )?;
    push_effect(
        engine,
        EffectEvent::SlowMotion(SlowMotionEvent::new(&params)),
    )?;
    Ok(())
}

fn current_post_process(engine: &Engine) -> Result<(f32, f32)> {
    let params = engine
        .get_global_component::<GameUIStateComponent>()?
        .post_process
        .clone();
    let guard = params
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok((guard.vignette_strength, guard.aberration_strength))
}

fn with_post_process(engine: &Engine, apply: impl FnOnce(&mut PostProcessParams)) -> Result<()> {
    let params = engine
        .get_global_component::<GameUIStateComponent>()?
        .post_process
        .clone();
    if let Ok(mut guard) = params.lock() {
        apply(&mut guard);
    }
    Ok(())
}

/// Moves new requests into `in_progress`, runs every in-progress handler and applies the frame's
/// camera shake / time scale.
fn effect_event_system(engine: &mut Engine) -> Result<()> {
    // Real (unscaled) delta: events age in real time so slow motion does not slow its own recovery.
    let real_delta = engine.get_global_component::<TimeComponent>()?.delta_time;

    // Take the queues out of the global so the handlers can borrow the engine freely.
    let (mut in_progress, queued) = {
        let state = engine.get_global_component_mut::<EffectQueueComponent>()?;
        for cooldown in state.cooldowns.iter_mut() {
            *cooldown = (*cooldown - real_delta).max(0.0);
        }
        (
            std::mem::take(&mut state.in_progress),
            std::mem::take(&mut state.queue),
        )
    };
    in_progress.extend(queued);

    let mut frame = FrameEffects::default();
    for event in &mut in_progress {
        event.handler(engine, real_delta, &mut frame)?;
    }

    // Events that just finished restore the stock post-process values.
    for event in in_progress.iter().filter(|event| event.is_finished()) {
        event.finish(engine)?;
    }
    in_progress.retain(|event| !event.is_finished());

    apply_camera_shake(engine, frame.shake)?;
    if (frame.time_scale - 1.0).abs() > f32::EPSILON {
        engine
            .get_global_component_mut::<TimeComponent>()?
            .set_delta_time(real_delta * frame.time_scale);
    }

    engine
        .get_global_component_mut::<EffectQueueComponent>()?
        .in_progress = in_progress;
    Ok(())
}

/// Applies the difference between last frame's and this frame's shake offset to the active camera.
fn apply_camera_shake(engine: &mut Engine, desired: Vector2f) -> Result<()> {
    let previous = engine
        .get_global_component::<EffectQueueComponent>()?
        .applied_shake;
    let delta = desired - previous;

    if delta != Vector2f::ZERO {
        for (_, transform, camera) in
            engine.iterate_two_components_mut::<TransformComponent, CameraComponent>()?
        {
            if !camera.enabled {
                continue;
            }
            let position = transform.position
                + transform.get_right_direction() * delta.x
                + transform.get_up_direction() * delta.y;
            transform.set_position(position);
            break;
        }
    }

    engine
        .get_global_component_mut::<EffectQueueComponent>()?
        .applied_shake = desired;
    Ok(())
}
