use glam::Vec3;

use crate::types::Aabb;

/// Small axis-aligned, non-rotating physics body used by world pickups.
#[derive(Clone, Copy, Debug)]
pub struct RigidBody {
    /// Center of mass in world space.
    pub position: Vec3,
    pub velocity: Vec3,
    pub half_extents: Vec3,
    pub on_ground: bool,
}

impl RigidBody {
    pub fn new(position: Vec3, half_extents: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            half_extents,
            on_ground: false,
        }
    }

    pub fn collider(self) -> Aabb {
        Aabb::from_center_size(self.position, self.half_extents * 2.0)
    }
}
