use std::collections::HashSet;

use glam::Vec3;
use winit::keyboard::KeyCode;

use crate::{
    camera::{jump_speed, movement_parameters, Camera, PLAYER_HEIGHT, PLAYER_RADIUS},
    types::Aabb,
};

pub fn update_player(
    player: &mut Camera,
    pressed: &HashSet<KeyCode>,
    jump_requested: bool,
    colliders: &[Aabb],
    dt: f32,
) {
    let forward = Vec3::new(player.yaw.cos(), 0.0, player.yaw.sin());
    let right = forward.cross(Vec3::Y);
    let mut wish = Vec3::ZERO;
    if pressed.contains(&KeyCode::KeyW) {
        wish += forward;
    }
    if pressed.contains(&KeyCode::KeyS) {
        wish -= forward;
    }
    if pressed.contains(&KeyCode::KeyD) {
        wish += right;
    }
    if pressed.contains(&KeyCode::KeyA) {
        wish -= right;
    }
    let running = pressed.contains(&KeyCode::ShiftLeft) || pressed.contains(&KeyCode::ShiftRight);
    let (speed, acceleration, friction, gravity) = movement_parameters(running);
    let current = Vec3::new(player.velocity.x, 0.0, player.velocity.z);
    let target = wish.normalize_or_zero() * speed;
    let difference = target - current;
    let rate = if wish.length_squared() > 0.0 {
        acceleration
    } else {
        friction
    };
    let horizontal = if difference.length() <= rate * dt {
        target
    } else {
        current + difference.normalize_or_zero() * rate * dt
    };
    player.velocity.x = horizontal.x;
    player.velocity.z = horizontal.z;

    if jump_requested && player.on_ground {
        player.velocity.y = jump_speed();
        player.on_ground = false;
    }
    player.velocity.y -= gravity * dt;
    move_axis(player, colliders, 0, player.velocity.x * dt);
    move_axis(player, colliders, 2, player.velocity.z * dt);
    let dy = player.velocity.y * dt;
    player.position.y += dy;
    player.on_ground = false;
    for collider in colliders {
        if intersects(player, *collider) {
            if dy > 0.0 {
                player.position.y = collider.min.y - PLAYER_HEIGHT;
            } else {
                player.position.y = collider.max.y;
                player.on_ground = true;
            }
            player.velocity.y = 0.0;
        }
    }
    if player.position.y <= 0.0 {
        player.position.y = 0.0;
        player.velocity.y = 0.0;
        player.on_ground = true;
    }
    player.position.x = player.position.x.clamp(-14.5, 14.5);
    player.position.z = player.position.z.clamp(-14.5, 14.5);
    player.animate(dt);
}

fn move_axis(player: &mut Camera, colliders: &[Aabb], axis: usize, delta: f32) {
    match axis {
        0 => player.position.x += delta,
        _ => player.position.z += delta,
    }
    for collider in colliders {
        if intersects(player, *collider) {
            let value = match (axis, delta > 0.0) {
                (0, true) => collider.min.x - PLAYER_RADIUS,
                (0, false) => collider.max.x + PLAYER_RADIUS,
                (_, true) => collider.min.z - PLAYER_RADIUS,
                (_, false) => collider.max.z + PLAYER_RADIUS,
            };
            if axis == 0 {
                player.position.x = value;
                player.velocity.x = 0.0;
            } else {
                player.position.z = value;
                player.velocity.z = 0.0;
            }
        }
    }
}

fn intersects(player: &Camera, collider: Aabb) -> bool {
    let min = Vec3::new(
        player.position.x - PLAYER_RADIUS,
        player.position.y,
        player.position.z - PLAYER_RADIUS,
    );
    let max = Vec3::new(
        player.position.x + PLAYER_RADIUS,
        player.position.y + PLAYER_HEIGHT,
        player.position.z + PLAYER_RADIUS,
    );
    min.x < collider.max.x
        && max.x > collider.min.x
        && min.y < collider.max.y
        && max.y > collider.min.y
        && min.z < collider.max.z
        && max.z > collider.min.z
}
