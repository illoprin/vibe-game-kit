use std::collections::HashSet;

use glam::Vec3;
use winit::keyboard::KeyCode;

use crate::{
    camera::{jump_speed, movement_parameters, Camera, PLAYER_HEIGHT, PLAYER_RADIUS},
    types::Aabb,
};

const MAX_STEP_HEIGHT: f32 = PLAYER_HEIGHT * 0.25;

pub fn update_player(player: &mut Camera, pressed: &HashSet<KeyCode>, colliders: &[Aabb], dt: f32) {
    let dt = dt.clamp(0.0, 0.05);
    if player.on_ground {
        player.reset_air_yaw();
    }
    let jump_now = pressed.contains(&KeyCode::Space) && player.on_ground;
    if jump_now {
        player.velocity.y = jump_speed();
        player.on_ground = false;
    }

    // WASD and sprint cannot add air acceleration. Existing momentum is only
    // redirected by mouse yaw while airborne.
    let grounded_control = player.on_ground;
    let forward = Vec3::new(player.yaw.cos(), 0.0, player.yaw.sin());
    let right = forward.cross(Vec3::Y);
    let mut wish = Vec3::ZERO;
    if grounded_control {
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
    }
    let running = grounded_control
        && wish.length_squared() > 0.0
        && (pressed.contains(&KeyCode::ShiftLeft) || pressed.contains(&KeyCode::ShiftRight));
    player.set_running(running);
    let (speed, acceleration, friction, gravity) = movement_parameters(running);
    let current = Vec3::new(player.velocity.x, 0.0, player.velocity.z);
    let target = wish.normalize_or_zero() * speed;
    if grounded_control {
        let rate = if wish.length_squared() > 0.0 {
            acceleration
        } else {
            friction
        };
        // Exponential approach gives acceleration and braking a soft ease-out.
        let blend = 1.0 - (-rate * dt).exp();
        let horizontal = current.lerp(target, blend);
        player.velocity.x = horizontal.x;
        player.velocity.z = horizontal.z;
    } else {
        player.steer_air_momentum_with_yaw();
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
            if player.on_ground && try_step_up(player, *collider, colliders) {
                continue;
            }
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

fn try_step_up(player: &mut Camera, obstacle: Aabb, colliders: &[Aabb]) -> bool {
    let rise = obstacle.max.y - player.position.y;
    if rise <= 0.0 || rise > MAX_STEP_HEIGHT {
        return false;
    }

    let mut stepped_position = player.position;
    stepped_position.y = obstacle.max.y;
    if colliders
        .iter()
        .any(|collider| intersects_at(stepped_position, *collider))
    {
        return false;
    }

    player.position = stepped_position;
    player.velocity.y = 0.0;
    true
}

fn intersects(player: &Camera, collider: Aabb) -> bool {
    intersects_at(player.position, collider)
}

fn intersects_at(position: Vec3, collider: Aabb) -> bool {
    let min = Vec3::new(
        position.x - PLAYER_RADIUS,
        position.y,
        position.z - PLAYER_RADIUS,
    );
    let max = Vec3::new(
        position.x + PLAYER_RADIUS,
        position.y + PLAYER_HEIGHT,
        position.z + PLAYER_RADIUS,
    );
    min.x < collider.max.x
        && max.x > collider.min.x
        && min.y < collider.max.y
        && max.y > collider.min.y
        && min.z < collider.max.z
        && max.z > collider.min.z
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(codes: &[KeyCode]) -> HashSet<KeyCode> {
        codes.iter().copied().collect()
    }

    fn floor() -> Aabb {
        Aabb::from_center_size(Vec3::new(0.0, -0.5, 0.0), Vec3::new(20.0, 1.0, 20.0))
    }

    #[test]
    fn holding_space_restarts_jump_after_landing() {
        let mut player = Camera::new();
        player.position.y = 0.0;
        player.velocity.y = 0.0;
        player.on_ground = true;
        let pressed = keys(&[KeyCode::Space]);
        let colliders = [floor()];
        let mut has_landed = false;
        let mut auto_jumped = false;
        for _ in 0..100 {
            update_player(&mut player, &pressed, &colliders, 0.01);
            if player.on_ground {
                has_landed = true;
            } else if has_landed && player.velocity.y > 0.0 {
                auto_jumped = true;
                break;
            }
        }
        assert!(has_landed, "the test jump should land on the floor");
        assert!(auto_jumped, "held jump should retrigger after landing");
    }

    #[test]
    fn airborne_player_cannot_accelerate_or_sprint() {
        let mut player = Camera::new();
        player.on_ground = false;
        player.velocity.x = 2.0;
        let pressed = keys(&[KeyCode::KeyW, KeyCode::ShiftLeft]);
        update_player(&mut player, &pressed, &[], 0.01);
        assert!((player.velocity.x - 2.0).abs() < 1e-6);
        assert_eq!(player.velocity.z, 0.0);
        assert_eq!(player.fov_degrees(), 90.0);
    }

    #[test]
    fn airborne_yaw_turn_rotates_momentum_without_wasd_control() {
        let mut player = Camera::new();
        player.on_ground = false;
        player.velocity = Vec3::new(0.0, 0.0, -2.0);
        player.apply_mouse_motion(785.398, 0.0, 1.0);
        update_player(
            &mut player,
            &keys(&[KeyCode::KeyW, KeyCode::ShiftLeft]),
            &[],
            0.001,
        );
        assert!(player.velocity.x > 1.9);
        assert!(player.velocity.z.abs() < 0.1);
    }

    #[test]
    fn player_steps_up_on_obstacles_below_quarter_height() {
        let mut player = Camera::new();
        player.position = Vec3::ZERO;
        player.on_ground = true;
        let floor = floor();
        let step = Aabb::from_center_size(Vec3::new(0.65, 0.15, 0.0), Vec3::new(0.5, 0.3, 1.0));
        let colliders = [floor, step];
        let pressed = keys(&[KeyCode::KeyW]);
        // Face +X for this collision test.
        player.yaw = 0.0;
        update_player(&mut player, &pressed, &colliders, 0.05);
        assert!(player.position.x > 0.0, "player should move onto the step");
        assert!((player.position.y - 0.3).abs() < 1e-5);
    }

    #[test]
    fn player_cannot_step_above_quarter_height() {
        let mut player = Camera::new();
        player.position = Vec3::ZERO;
        player.on_ground = true;
        let step_height = MAX_STEP_HEIGHT + 0.01;
        let step = Aabb::from_center_size(
            Vec3::new(0.65, step_height * 0.5, 0.0),
            Vec3::new(0.5, step_height, 1.0),
        );
        let colliders = [floor(), step];
        player.yaw = 0.0;
        update_player(&mut player, &keys(&[KeyCode::KeyW]), &colliders, 0.05);
        assert_eq!(player.position.y, 0.0);
        assert!(player.position.x < step.min.x);
    }
}
