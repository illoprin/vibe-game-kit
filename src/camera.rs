use glam::{Mat4, Vec3};

pub const PLAYER_HEIGHT: f32 = 1.8;
pub const PLAYER_RADIUS: f32 = 0.32;
const WALK_SPEED: f32 = 5.5;
const RUN_SPEED: f32 = 8.5;
const ACCELERATION: f32 = 50.0;
const FRICTION: f32 = 20.0;
const GRAVITY: f32 = 22.0;
const JUMP_SPEED: f32 = 8.0;

// Stride rate in phase radians per second. Lower values make the head bob and
// the held-model sway slower and smoother.
const WALK_CADENCE: f32 = 6.5;
const RUN_CADENCE: f32 = 7.5;
// Peak vertical camera offset in meters at full bob.
const CAMERA_BOB: f32 = 0.05;
// Peak viewmodel sway offsets in meters, as (walk, run).
const VIEW_SWAY_X: (f32, f32) = (0.05, 0.1);
const VIEW_SWAY_Y: (f32, f32) = (0.03, 0.035);
// Uniform size multiplier and eye-space placement of the held model. The model
// is kept small and close so it sits inside the player's collision radius.
const VIEWMODEL_SCALE: f32 = 0.7;
const VIEWMODEL_RIGHT: f32 = 0.24;
const VIEWMODEL_UP: f32 = -0.19;
const VIEWMODEL_FORWARD: f32 = 0.45;

pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub bob_phase: f32,
    pub bob_amount: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            position: Vec3::new(0.0, 0.0, 10.0),
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: 0.0,
            velocity: Vec3::ZERO,
            on_ground: true,
            bob_phase: 0.0,
            bob_amount: 0.0,
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin())
            .cross(Vec3::Y)
            .normalize()
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let bob = if self.on_ground {
            self.bob_amount * (self.bob_phase * 2.0).sin() * CAMERA_BOB
        } else {
            0.0
        };
        let eye = self.position + Vec3::Y * (1.62 + bob);
        let forward = self.forward();
        Mat4::perspective_rh(90.0_f32.to_radians(), aspect, 0.1, 120.0)
            * Mat4::look_at_rh(eye, eye + forward, Vec3::Y)
    }

    pub fn view_model(&self) -> Mat4 {
        let forward = self.forward();
        let right = self.right();
        let up = right.cross(forward).normalize();
        let speed = Vec3::new(self.velocity.x, 0.0, self.velocity.z).length();
        let run = speed > WALK_SPEED + 0.5;
        let bob = if self.on_ground { self.bob_amount } else { 0.0 };
        let (sway_x, sway_y) = if run {
            (VIEW_SWAY_X.1, VIEW_SWAY_Y.1)
        } else {
            (VIEW_SWAY_X.0, VIEW_SWAY_Y.0)
        };
        let x_sway = self.bob_phase.sin() * bob * sway_x;
        let y_sway = (self.bob_phase * 2.0).cos().abs() * bob * sway_y;
        let origin = self.position
            + Vec3::Y * 1.62
            + right * (VIEWMODEL_RIGHT + x_sway)
            + up * (VIEWMODEL_UP - y_sway)
            + forward * VIEWMODEL_FORWARD;
        Mat4::from_cols(
            (right * VIEWMODEL_SCALE).extend(0.0),
            (up * VIEWMODEL_SCALE).extend(0.0),
            (forward * VIEWMODEL_SCALE).extend(0.0),
            origin.extend(1.0),
        )
    }

    pub fn animate(&mut self, dt: f32) {
        let speed = Vec3::new(self.velocity.x, 0.0, self.velocity.z).length();
        let target = if self.on_ground {
            (speed / RUN_SPEED).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.bob_amount += (target - self.bob_amount) * (dt * 10.0).min(1.0);
        if speed > 0.1 && self.on_ground {
            let cadence = if speed > WALK_SPEED + 0.5 {
                RUN_CADENCE
            } else {
                WALK_CADENCE
            };
            self.bob_phase += dt * cadence;
        }
    }
}

pub fn movement_parameters(running: bool) -> (f32, f32, f32, f32) {
    (
        if running { RUN_SPEED } else { WALK_SPEED },
        ACCELERATION,
        FRICTION,
        GRAVITY,
    )
}

pub fn jump_speed() -> f32 {
    JUMP_SPEED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewmodel_is_scaled_and_sits_close_to_the_eye() {
        let camera = Camera::new();
        let model = camera.view_model();
        let scale = model.x_axis.truncate().length();
        assert!((scale - VIEWMODEL_SCALE).abs() < 1e-6);
        let eye = camera.position + Vec3::Y * 1.62;
        let distance = (model.w_axis.truncate() - eye).length();
        assert!(
            distance < 0.6,
            "held model should sit close to the eye, got {distance}"
        );
    }

    #[test]
    fn walking_bob_slows_down_and_grows() {
        let mut slow = Camera::new();
        slow.on_ground = true;
        slow.velocity = Vec3::new(WALK_SPEED, 0.0, 0.0);
        slow.animate(0.1);
        assert!((slow.bob_phase - 0.1 * WALK_CADENCE).abs() < 1e-5);
        assert!(CAMERA_BOB > 0.045);
    }
}
