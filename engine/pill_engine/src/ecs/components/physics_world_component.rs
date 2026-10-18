use crate::ecs::components::{Component, GlobalComponent, GlobalComponentStorage};
use crate::ecs::{EntityHandle, SceneHandle};

use pill_core::PillTypeMapKey;
use pill_core::{ErrorContext, Result};
use rapier3d::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};

/// Which edge of the collision interval was generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollisionPhase {
    Started,
    Stopped,
}

/// Engine entity and named part associated with a Rapier collider.
#[derive(Clone)]
pub struct ColliderIdentity {
    pub scene: SceneHandle,
    pub entity: EntityHandle,
    pub part: String,
    pub collider: ColliderHandle,
}

/// Named sensor zones that overlap while two physical colliders touch.
/// `a` and `b` are oriented to match `PhysicsCollisionEvent::a` and `::b`.
#[derive(Clone)]
pub struct ColliderZoneContact {
    pub a: ColliderIdentity,
    pub b: ColliderIdentity,
}

/// Game-facing collision event enriched after Rapier has solved the physics step.
///
/// - Sensor overlap: `force_magnitude` is `None`, `zone_contacts` is empty.
/// - Solid-body contact: `force_magnitude` is the sum of contact impulse
///   magnitudes divided by the fixed physics timestep (can be 0.0).
/// - Solid-body Started: `zone_contacts` lists the sensor pairs currently
///   overlapping *between these two entities*, so games can classify an
///   impact by region without correlating separate event streams.
///
/// This is one event per actual Rapier collider-pair start/stop, NOT one event
/// per zone. Force represents the physical contact, never a sensor force.
#[derive(Clone)]
pub struct PhysicsCollisionEvent {
    pub phase: CollisionPhase,
    pub a: ColliderIdentity,
    pub b: ColliderIdentity,
    pub force_magnitude: Option<Real>,
    pub zone_contacts: Vec<ColliderZoneContact>,
}

/// Global Rapier physics world with a buffered, game-facing collision event queue.
pub struct PhysicsWorldComponent {
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,
    pub gravity: Vector,
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    pub narrow_phase: NarrowPhase,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub physics_hooks: (),
    pub event_handler: ChannelEventCollector,
    pub time_accumulated: f32,

    collision_event_receiver: Receiver<CollisionEvent>,
    // Keep the receiving side alive; callers can add force-event support later.
    _contact_force_event_receiver: Receiver<ContactForceEvent>,
    collider_owners: HashMap<ColliderHandle, ColliderIdentity>,
    pending_collision_events: Vec<PhysicsCollisionEvent>,
}

impl PhysicsWorldComponent {
    pub fn new(time_step: f32, meters_per_world_unit: f32) -> Self {
        let (collision_sender, collision_event_receiver) = channel();
        let (force_sender, contact_force_event_receiver) = channel();

        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            gravity: vector![0.0, -9.81 / meters_per_world_unit, 0.0].into(),
            integration_parameters: IntegrationParameters {
                dt: time_step,
                ..Default::default()
            },
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            physics_hooks: (),
            event_handler: ChannelEventCollector::new(collision_sender, force_sender),
            time_accumulated: 0.0,
            collision_event_receiver,
            _contact_force_event_receiver: contact_force_event_receiver,
            collider_owners: HashMap::new(),
            pending_collision_events: Vec::new(),
        }
    }

    pub fn set_gravity(&mut self, gravity: Vector) {
        self.gravity = gravity;
    }

    pub fn set_timestep(&mut self, time_step: f32) {
        self.integration_parameters.dt = time_step;
    }

    pub(crate) fn register_collider(
        &mut self,
        collider: ColliderHandle,
        scene: SceneHandle,
        entity: EntityHandle,
        part: impl Into<String>,
    ) {
        self.collider_owners.insert(
            collider,
            ColliderIdentity {
                scene,
                entity,
                part: part.into(),
                collider,
            },
        );
    }

    pub(crate) fn unregister_collider(&mut self, collider: ColliderHandle) {
        self.collider_owners.remove(&collider);
        // Do not hand game code stale events for a destroyed collider.
        self.pending_collision_events
            .retain(|event| event.a.collider != collider && event.b.collider != collider);
    }

    /// Take collision events since the last drain. Does not repeat old collisions.
    /// Called during Game-phase systems, these are events from the previous PostGame physics step.
    pub fn drain_collision_events(&mut self) -> Vec<PhysicsCollisionEvent> {
        std::mem::take(&mut self.pending_collision_events)
    }

    /// Read pending collisions without consuming them.
    pub fn collision_events(&self) -> &[PhysicsCollisionEvent] {
        &self.pending_collision_events
    }

    /// Return currently overlapping *sensor vs sensor* zones for the two
    /// entities involved in a solid-body contact. The Rapier narrow phase is
    /// already populated for the just-completed fixed step, so no second
    /// collision query or additional force-event subscription is necessary.
    fn overlapping_zones(
        &self,
        a: &ColliderIdentity,
        b: &ColliderIdentity,
    ) -> Vec<ColliderZoneContact> {
        let mut zones = Vec::new();
        for (first_handle, second_handle, overlapping) in self.narrow_phase.intersection_pairs() {
            if !overlapping {
                continue;
            }
            let (Some(first), Some(second)) = (
                self.collider_owners.get(&first_handle),
                self.collider_owners.get(&second_handle),
            ) else {
                continue;
            };
            // Explicitly require two sensors: an overlap with the solid hull
            // is not sufficient to say which named sections hit each other.
            let (Some(first_collider), Some(second_collider)) = (
                self.collider_set.get(first_handle),
                self.collider_set.get(second_handle),
            ) else {
                continue;
            };
            if !first_collider.is_sensor() || !second_collider.is_sensor() {
                continue;
            }

            if first.scene == a.scene
                && first.entity == a.entity
                && second.scene == b.scene
                && second.entity == b.entity
            {
                zones.push(ColliderZoneContact {
                    a: first.clone(),
                    b: second.clone(),
                });
            } else if second.scene == a.scene
                && second.entity == a.entity
                && first.scene == b.scene
                && first.entity == b.entity
            {
                zones.push(ColliderZoneContact {
                    a: second.clone(),
                    b: first.clone(),
                });
            }
        }
        zones
    }

    pub fn step(&mut self) {
        self.physics_pipeline.step(
            self.gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            &self.physics_hooks,
            &self.event_handler,
        );

        // Extract events after EACH fixed timestep. Multiple physics substeps in one
        // rendered frame are retained until the game drains the queue.
        while let Ok(event) = self.collision_event_receiver.try_recv() {
            let (phase, collider_a, collider_b) = match event {
                CollisionEvent::Started(a, b, _) => (CollisionPhase::Started, a, b),
                CollisionEvent::Stopped(a, b, _) => (CollisionPhase::Stopped, a, b),
            };

            let (Some(a), Some(b)) = (
                self.collider_owners.get(&collider_a).cloned(),
                self.collider_owners.get(&collider_b).cloned(),
            ) else {
                // External Rapier colliders that weren't created by ColliderComponent.
                continue;
            };

            // An entity's own sections are not meaningful game collisions.
            if a.scene == b.scene && a.entity == b.entity {
                continue;
            }

            // Collision start events are emitted during narrow-phase detection,
            // before the solver knows the contact force. Querying AFTER step()
            // gives the solved impulse for that exact collider pair. Sensors do
            // not have solver impulses; they always report None.
            let force_magnitude = if phase == CollisionPhase::Started {
                self.narrow_phase
                    .contact_pair(collider_a, collider_b)
                    .filter(|pair| pair.has_any_active_contact())
                    .map(|pair| {
                        pair.total_impulse_magnitude()
                            / self.integration_parameters.dt.max(f32::EPSILON)
                    })
            } else {
                None
            };

            // Only enrich real physical contacts. For sensor-only events, the
            // old Started/Stopped API remains available with no impact force.
            let zone_contacts = if force_magnitude.is_some() {
                self.overlapping_zones(&a, &b)
            } else {
                Vec::new()
            };

            self.pending_collision_events.push(PhysicsCollisionEvent {
                phase,
                a,
                b,
                force_magnitude,
                zone_contacts,
            });
        }
    }
}

impl Default for PhysicsWorldComponent {
    fn default() -> Self {
        const TIME_STEP: f32 = 1.0 / 60.0;
        const METERS_PER_WORLD_UNIT: f32 = 1.0;
        Self::new(TIME_STEP, METERS_PER_WORLD_UNIT)
    }
}

impl Component for PhysicsWorldComponent {}

impl PillTypeMapKey for PhysicsWorldComponent {
    type Storage = GlobalComponentStorage<PhysicsWorldComponent>;
}

impl GlobalComponent for PhysicsWorldComponent {}
