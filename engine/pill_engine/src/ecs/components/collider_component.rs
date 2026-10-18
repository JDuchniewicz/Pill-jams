use crate::{
    ecs::{
        Component, ComponentStorage, DeferredUpdateComponent, DeferredUpdateComponentRequest,
        DeferredUpdateManagerPointer, EntityHandle, PhysicsWorldComponent, RigidBodyComponent,
        SceneHandle,
    },
    engine::Engine,
};

use pill_core::{get_type_name, PillStyle, PillTypeMapKey, Vector3f};
use pill_core::{ErrorContext, Result};
use rapier3d::prelude::*;

const DEFERRED_REQUEST_VARIANT_ADD: usize = 0;

/// Named collider attached to the same rigid body as the primary collider.
/// By default, parts are massless sensors with collision start/stop events enabled.
#[derive(Clone)]
pub struct ColliderPart {
    pub name: String,
    pub shape: SharedShape,
    pub position: Pose,
    pub friction: Real,
    pub restitution: Real,
    pub mass: Real,
    pub is_sensor: bool,
    pub collision_groups: InteractionGroups,
    pub solver_groups: InteractionGroups,
    pub active_events: ActiveEvents,
}

impl ColliderPart {
    pub fn new(name: impl Into<String>, shape: SharedShape) -> Self {
        Self {
            name: name.into(),
            shape,
            position: Pose::identity(),
            friction: 0.5,
            restitution: 0.0,
            mass: 0.0,
            is_sensor: true,
            collision_groups: InteractionGroups::all(),
            solver_groups: InteractionGroups::all(),
            active_events: ActiveEvents::COLLISION_EVENTS,
        }
    }

    pub fn position(mut self, position: Pose) -> Self {
        self.position = position;
        self
    }

    pub fn translation(mut self, translation: Vector3f) -> Self {
        self.position = Pose::translation(translation.x, translation.y, translation.z);
        self
    }

    pub fn sensor(mut self, is_sensor: bool) -> Self {
        self.is_sensor = is_sensor;
        self
    }

    pub fn mass(mut self, mass: Real) -> Self {
        self.mass = mass;
        self
    }

    pub fn friction(mut self, friction: Real) -> Self {
        self.friction = friction;
        self
    }

    pub fn restitution(mut self, restitution: Real) -> Self {
        self.restitution = restitution;
        self
    }

    pub fn collision_groups(mut self, groups: InteractionGroups) -> Self {
        self.collision_groups = groups;
        self
    }

    pub fn solver_groups(mut self, groups: InteractionGroups) -> Self {
        self.solver_groups = groups;
        self
    }

    pub fn active_events(mut self, events: ActiveEvents) -> Self {
        self.active_events = events;
        self
    }
}

impl From<ColliderPart> for Collider {
    fn from(part: ColliderPart) -> Self {
        ColliderBuilder::new(part.shape)
            .position(part.position)
            .friction(part.friction)
            .restitution(part.restitution)
            .mass(part.mass)
            .sensor(part.is_sensor)
            .collision_groups(part.collision_groups)
            .solver_groups(part.solver_groups)
            .active_events(part.active_events)
            .build()
    }
}

/// Existing hull collider, optionally with additional named colliders.
/// The original builder API still creates the primary `"hull"` collider.
#[derive(Clone)]
#[readonly::make]
pub struct ColliderComponent {
    #[readonly]
    pub shape: SharedShape,
    #[readonly]
    pub position: Pose,
    #[readonly]
    pub friction: Real,
    #[readonly]
    pub restitution: Real,
    #[readonly]
    pub mass: Real,
    #[readonly]
    pub is_sensor: bool,
    #[readonly]
    pub collision_groups: InteractionGroups,
    #[readonly]
    pub solver_groups: InteractionGroups,
    #[readonly]
    pub active_events: ActiveEvents,
    #[readonly]
    pub parts: Vec<ColliderPart>,

    collider_handle: Option<ColliderHandle>,
    part_handles: Vec<ColliderHandle>,
    scene_handle: Option<SceneHandle>,
    entity_handle: Option<EntityHandle>,
    deferred_update_manager: Option<DeferredUpdateManagerPointer>,
}

impl ColliderComponent {
    pub fn new() -> Self {
        Self {
            shape: SharedShape::cuboid(0.5, 0.5, 0.5),
            position: Pose::identity(),
            friction: 0.5,
            restitution: 0.0,
            mass: 1.0,
            is_sensor: false,
            collision_groups: InteractionGroups::all(),
            solver_groups: InteractionGroups::all(),
            active_events: ActiveEvents::empty(),
            parts: Vec::new(),
            collider_handle: None,
            part_handles: Vec::new(),
            scene_handle: None,
            entity_handle: None,
            deferred_update_manager: None,
        }
    }

    pub fn builder() -> ColliderComponentBuilder {
        ColliderComponentBuilder::default()
    }

    fn post_deferred_update_request(&mut self, request_variant: usize) {
        if let Some(update_manager) = &mut self.deferred_update_manager {
            let entity_handle = self.entity_handle.expect(
                "Critical: Cannot post deferred update request. No EntityHandle set in Component",
            );
            let scene_handle = self.scene_handle.expect(
                "Critical: Cannot post deferred update request. No SceneHandle set in Component",
            );
            update_manager.post_update_request(
                DeferredUpdateComponentRequest::<ColliderComponent>::new(
                    entity_handle,
                    scene_handle,
                    request_variant,
                ),
            );
        }
    }
}

impl Default for ColliderComponent {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ColliderComponentBuilder {
    component: ColliderComponent,
}

impl ColliderComponentBuilder {
    pub fn new() -> Self {
        Self {
            component: ColliderComponent::new(),
        }
    }

    pub fn default() -> Self {
        Self::new()
    }

    pub fn shape(mut self, shape: SharedShape) -> Self {
        self.component.shape = shape;
        self
    }

    pub fn friction(mut self, friction: Real) -> Self {
        self.component.friction = friction;
        self
    }

    pub fn restitution(mut self, restitution: Real) -> Self {
        self.component.restitution = restitution;
        self
    }

    pub fn mass(mut self, mass: Real) -> Self {
        self.component.mass = mass;
        self
    }

    pub fn sensor(mut self, is_sensor: bool) -> Self {
        self.component.is_sensor = is_sensor;
        self
    }

    pub fn collision_groups(mut self, collision_groups: InteractionGroups) -> Self {
        self.component.collision_groups = collision_groups;
        self
    }

    pub fn solver_groups(mut self, solver_groups: InteractionGroups) -> Self {
        self.component.solver_groups = solver_groups;
        self
    }

    pub fn active_events(mut self, events: ActiveEvents) -> Self {
        self.component.active_events = events;
        self
    }

    /// Add another collider to this entity's rigid body.
    pub fn part(mut self, part: ColliderPart) -> Self {
        assert!(
            !part.name.is_empty(),
            "Collider part name must not be empty"
        );
        assert!(
            part.name != "hull",
            "'hull' is reserved for the primary collider"
        );
        assert!(
            self.component
                .parts
                .iter()
                .all(|existing| existing.name != part.name),
            "Collider part names must be unique within one entity"
        );
        self.component.parts.push(part);
        self
    }

    pub fn position(mut self, position: Pose) -> Self {
        self.component.position = position;
        self
    }

    pub fn translation(mut self, translation: Vector3f) -> Self {
        self.component.position = Pose::translation(translation.x, translation.y, translation.z);
        self
    }

    pub fn build(self) -> ColliderComponent {
        self.component
    }
}

impl PillTypeMapKey for ColliderComponent {
    type Storage = ComponentStorage<ColliderComponent>;
}

impl From<ColliderComponent> for Collider {
    fn from(collider: ColliderComponent) -> Self {
        ColliderBuilder::new(collider.shape)
            .position(collider.position)
            .friction(collider.friction)
            .restitution(collider.restitution)
            .mass(collider.mass)
            .sensor(collider.is_sensor)
            .collision_groups(collider.collision_groups)
            .solver_groups(collider.solver_groups)
            .active_events(collider.active_events)
            .build()
    }
}

impl Component for ColliderComponent {
    fn initialize(&mut self, engine: &mut Engine) -> Result<()> {
        let deferred_update_component = engine
            .get_global_component_mut::<DeferredUpdateComponent>()
            .expect("Critical: No DeferredUpdateComponent");
        self.deferred_update_manager =
            Some(deferred_update_component.borrow_deferred_update_manager());
        Ok(())
    }

    fn pass_handles(&mut self, scene_handle: SceneHandle, entity_handle: EntityHandle) {
        self.scene_handle = Some(scene_handle);
        self.entity_handle = Some(entity_handle);
        self.post_deferred_update_request(DEFERRED_REQUEST_VARIANT_ADD);
    }

    fn deferred_update(&mut self, engine: &mut Engine, request: usize) -> Result<()> {
        match request {
            DEFERRED_REQUEST_VARIANT_ADD => {
                if self.collider_handle.is_some() {
                    return Ok(());
                }

                let entity_handle = self.entity_handle.expect("ColliderComponent has no entity");
                let scene_handle = self.scene_handle.expect("ColliderComponent has no scene");

                // The rigid body request is processed before this collider request, as in
                // Pill's existing deferred-update path. Standalone colliders remain supported.
                let rigid_body_handle: Option<RigidBodyHandle> = {
                    let scene = engine.scene_manager.get_scene(scene_handle)?;
                    let storage = scene.get_component_storage::<RigidBodyComponent>()?;
                    storage
                        .data
                        .get(entity_handle.0.index as usize)
                        .and_then(|slot| slot.as_ref())
                        .and_then(|rb| rb.get_rigid_body_handle())
                };

                let physics_world = engine.get_global_component_mut::<PhysicsWorldComponent>()?;
                let primary: Collider = self.clone().into();
                let primary_handle = match rigid_body_handle {
                    Some(parent) => physics_world.collider_set.insert_with_parent(
                        primary,
                        parent,
                        &mut physics_world.rigid_body_set,
                    ),
                    None => physics_world.collider_set.insert(primary),
                };
                physics_world.register_collider(
                    primary_handle,
                    scene_handle,
                    entity_handle,
                    "hull",
                );
                self.collider_handle = Some(primary_handle);

                for part in &self.parts {
                    let collider: Collider = part.clone().into();
                    let handle = match rigid_body_handle {
                        Some(parent) => physics_world.collider_set.insert_with_parent(
                            collider,
                            parent,
                            &mut physics_world.rigid_body_set,
                        ),
                        None => physics_world.collider_set.insert(collider),
                    };
                    physics_world.register_collider(
                        handle,
                        scene_handle,
                        entity_handle,
                        part.name.clone(),
                    );
                    self.part_handles.push(handle);
                }
            }
            _ => {
                panic!(
                    "Critical: Processing deferred update request with value {} in {} failed. Handling is not implemented",
                    request,
                    get_type_name::<Self>().specific_object_style()
                );
            }
        }
        Ok(())
    }

    fn destroy(
        &mut self,
        engine: &mut Engine,
        _scene_handle: SceneHandle,
        _entity_handle: EntityHandle,
    ) -> Result<()> {
        let mut handles = std::mem::take(&mut self.part_handles);
        if let Some(primary) = self.collider_handle.take() {
            handles.push(primary);
        }

        if !handles.is_empty() {
            let physics_world = engine.get_global_component_mut::<PhysicsWorldComponent>()?;
            for handle in handles {
                physics_world.unregister_collider(handle);
                physics_world.collider_set.remove(
                    handle,
                    &mut physics_world.island_manager,
                    &mut physics_world.rigid_body_set,
                    true,
                );
            }
        }
        Ok(())
    }
}
