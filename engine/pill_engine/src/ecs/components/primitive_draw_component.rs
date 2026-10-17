use crate::ecs::components::{GlobalComponent, GlobalComponentStorage};

use pill_core::{Color, PillTypeMapKey, Vector3f};

use std::cell::{Cell, RefCell};

/// Style of a primitive draw command.
#[derive(Clone, Copy, Debug)]
pub struct PrimitiveStyle {
    /// Linear RGBA color.
    pub color: [f32; 4],
    /// `true` = occluded by scene geometry (depth-tested); `false` = always drawn on top.
    pub depth_test: bool,
}

impl PrimitiveStyle {
    /// Depth-tested style with an opaque color.
    pub fn new(color: Color) -> Self {
        Self {
            color: [color.x, color.y, color.z, 1.0],
            depth_test: true,
        }
    }

    /// Always-visible style (ignores depth), as commonly used for debug overlays.
    pub fn overlay(color: Color) -> Self {
        Self {
            color: [color.x, color.y, color.z, 1.0],
            depth_test: false,
        }
    }

    /// Replaces the alpha component.
    pub fn with_alpha(mut self, alpha: f32) -> Self {
        self.color[3] = alpha;
        self
    }
}

impl From<Color> for PrimitiveStyle {
    fn from(color: Color) -> Self {
        Self::new(color)
    }
}

impl From<[f32; 3]> for PrimitiveStyle {
    fn from(color: [f32; 3]) -> Self {
        Self {
            color: [color[0], color[1], color[2], 1.0],
            depth_test: true,
        }
    }
}

impl From<(f32, f32, f32)> for PrimitiveStyle {
    fn from(color: (f32, f32, f32)) -> Self {
        Self {
            color: [color.0, color.1, color.2, 1.0],
            depth_test: true,
        }
    }
}

// --- Commands ---

#[derive(Clone, Copy)]
pub(crate) struct PrimitiveLine {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub color: [f32; 4],
    pub depth_test: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct PrimitivePoint {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub depth_test: bool,
}

/// Per-frame queue of line/point draw commands, consumed by `PassPrimitives`.
///
/// Commands are submitted through `Engine::draw_line` / `Engine::draw_point` and are cleared
/// automatically after the frame they were submitted in has been rendered.
///
/// Interior mutability keeps the game-facing API `&self`-friendly while passes only get
/// read-only access to globals.
pub struct PrimitiveDrawComponent {
    lines: RefCell<Vec<PrimitiveLine>>,
    points: RefCell<Vec<PrimitivePoint>>,
    line_capacity: usize,
    point_capacity: usize,
    capacity_warned: Cell<bool>,
}

impl PrimitiveDrawComponent {
    pub(crate) fn new(line_capacity: usize, point_capacity: usize) -> Self {
        Self {
            lines: RefCell::new(Vec::with_capacity(line_capacity)),
            points: RefCell::new(Vec::with_capacity(point_capacity)),
            line_capacity,
            point_capacity,
            capacity_warned: Cell::new(false),
        }
    }

    fn warn_capacity_once(&self, kind: &str, capacity: usize) {
        if !self.capacity_warned.replace(true) {
            log::warn!(
                "PrimitiveDrawComponent: {kind} capacity {capacity} reached; extra commands are dropped"
            );
        }
    }

    pub(crate) fn push_line(&self, from: Vector3f, to: Vector3f, style: PrimitiveStyle) {
        let mut lines = self.lines.borrow_mut();
        if lines.len() >= self.line_capacity {
            self.warn_capacity_once("line", self.line_capacity);
            return;
        }
        lines.push(PrimitiveLine {
            from: [from.x, from.y, from.z],
            to: [to.x, to.y, to.z],
            color: style.color,
            depth_test: style.depth_test,
        });
    }

    pub(crate) fn push_point(&self, position: Vector3f, style: PrimitiveStyle) {
        let mut points = self.points.borrow_mut();
        if points.len() >= self.point_capacity {
            self.warn_capacity_once("point", self.point_capacity);
            return;
        }
        points.push(PrimitivePoint {
            position: [position.x, position.y, position.z],
            color: style.color,
            depth_test: style.depth_test,
        });
    }

    /// Drains the queued lines (called once per frame by `PassPrimitives`).
    pub(crate) fn take_lines(&self) -> Vec<PrimitiveLine> {
        std::mem::take(&mut *self.lines.borrow_mut())
    }

    /// Drains the queued points (called once per frame by `PassPrimitives`).
    pub(crate) fn take_points(&self) -> Vec<PrimitivePoint> {
        std::mem::take(&mut *self.points.borrow_mut())
    }
}

impl PillTypeMapKey for PrimitiveDrawComponent {
    type Storage = GlobalComponentStorage<PrimitiveDrawComponent>;
}

impl GlobalComponent for PrimitiveDrawComponent {}
