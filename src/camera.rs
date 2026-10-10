use glam::{Mat4, Vec3};

pub const PLAYER_HEIGHT: f32 = 1.8;
pub const PLAYER_RADIUS: f32 = 0.32;
const WALK_SPEED: f32 = 5.5;
const RUN_SPEED: f32 = 8.5;
const ACCELERATION: f32 = 12.0;
const FRICTION: f32 = 10.0;
const GRAVITY: f32 = 22.0;
const JUMP_SPEED: f32 = 8.0;
const BASE_FOV: f32 = 90.0;
const RUN_FOV_BONUS: f32 = 12.0;
const CAMERA_FOLLOW_RATE: f32 = 24.0;

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

pub struct Camera {
    /// Collision/physics position (the player's feet).
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Vec3,
    pub on_ground: bool,
    pub bob_phase: f32,
    pub bob_amount: f32,
    visual_position: Vec3,
    air_yaw: f32,
    recoil_yaw: f32,
    recoil_yaw_target: f32,
    recoil_pitch: f32,
    recoil_pitch_target: f32,
    recoil_visual: f32,
    recoil_visual_target: f32,
    recoil_snappiness: f32,
    recoil_return_speed: f32,
    run_amount: f32,
    run_from: f32,
    run_target: f32,
    run_elapsed: f32,
    running: bool,
}

impl Camera {
    pub fn new() -> Self {
        let position = Vec3::new(0.0, 0.0, 10.0);
        Self {
            position,
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: 0.0,
            velocity: Vec3::ZERO,
            on_ground: true,
            bob_phase: 0.0,
            bob_amount: 0.0,
            visual_position: position,
            air_yaw: -std::f32::consts::FRAC_PI_2,
            recoil_yaw: 0.0,
            recoil_yaw_target: 0.0,
            recoil_pitch: 0.0,
            recoil_pitch_target: 0.0,
            recoil_visual: 0.0,
            recoil_visual_target: 0.0,
            recoil_snappiness: 25.0,
            recoil_return_speed: 8.0,
            run_amount: 0.0,
            run_from: 0.0,
            run_target: 0.0,
            run_elapsed: 0.0,
            running: false,
        }
    }

    pub fn apply_mouse_motion(&mut self, dx: f64, dy: f64, sensitivity: f32) {
        let scale = 0.002 * sensitivity.clamp(0.1, 3.0);
        // Keep aiming immediate for precise shooter controls. Camera position,
        // head bob and held-model motion remain eased independently.
        self.yaw += dx as f32 * scale;
        self.pitch = (self.pitch - dy as f32 * scale).clamp(-1.54, 1.54);
    }

    pub fn reset_air_yaw(&mut self) {
        self.air_yaw = self.yaw_angle();
    }

    pub fn steer_air_momentum_with_yaw(&mut self) {
        let yaw = self.yaw_angle();
        let yaw_delta = angle_delta(self.air_yaw, yaw);
        self.air_yaw = yaw;

        // Rotate existing horizontal momentum with the look direction, but do
        // not add air acceleration from WASD or sprint.
        let (sin, cos) = yaw_delta.sin_cos();
        let x = self.velocity.x;
        let z = self.velocity.z;
        self.velocity.x = cos * x - sin * z;
        self.velocity.z = sin * x + cos * z;
    }

    pub fn apply_weapon_recoil(
        &mut self,
        pitch_degrees: f32,
        yaw_degrees: f32,
        visual_kickback: f32,
        snappiness: f32,
        return_speed: f32,
    ) {
        self.recoil_pitch_target += pitch_degrees.to_radians();
        self.recoil_yaw_target += yaw_degrees.to_radians();
        self.recoil_visual_target = self.recoil_visual_target.max(visual_kickback.max(0.0));
        self.recoil_snappiness = snappiness.max(0.1);
        self.recoil_return_speed = return_speed.max(0.1);
    }

    pub fn yaw_angle(&self) -> f32 {
        self.yaw + self.recoil_yaw
    }

    pub fn pitch_angle(&self) -> f32 {
        (self.pitch + self.recoil_pitch).clamp(-1.54, 1.54)
    }

    pub fn set_running(&mut self, running: bool) {
        if self.running == running {
            return;
        }
        self.running = running;
        self.run_from = self.run_amount;
        self.run_target = if running { 1.0 } else { 0.0 };
        self.run_elapsed = 0.0;
    }

    pub fn render_position(&self) -> Vec3 {
        self.visual_position
    }

    pub fn fov_degrees(&self) -> f32 {
        BASE_FOV + RUN_FOV_BONUS * self.run_amount
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw_angle().cos() * self.pitch_angle().cos(),
            self.pitch_angle().sin(),
            self.yaw_angle().sin() * self.pitch_angle().cos(),
        )
        .normalize()
    }

    pub fn right(&self) -> Vec3 {
        Vec3::new(self.yaw_angle().cos(), 0.0, self.yaw_angle().sin())
            .cross(Vec3::Y)
            .normalize()
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let bob = self.bob_amount * (self.bob_phase * 2.0).sin() * CAMERA_BOB;
        let eye = self.visual_position + Vec3::Y * (1.62 + bob);
        let forward = self.forward();
        Mat4::perspective_rh(self.fov_degrees().to_radians(), aspect, 0.1, 120.0)
            * Mat4::look_at_rh(eye, eye + forward, Vec3::Y)
    }

    pub fn view_model_with_offset(&self, offset: [f32; 3], switch_drop: f32) -> Mat4 {
        let forward = self.forward();
        let right = self.right();
        let up = right.cross(forward).normalize();
        // bob_amount eases toward zero in the air instead of snapping on takeoff.
        let bob = self.bob_amount;
        let sway_x = VIEW_SWAY_X.0 + (VIEW_SWAY_X.1 - VIEW_SWAY_X.0) * self.run_amount;
        let sway_y = VIEW_SWAY_Y.0 + (VIEW_SWAY_Y.1 - VIEW_SWAY_Y.0) * self.run_amount;
        let x_sway = self.bob_phase.sin() * bob * sway_x;
        let y_sway = (self.bob_phase * 2.0).cos().abs() * bob * sway_y;
        let origin = self.visual_position
            + Vec3::Y * 1.62
            + right * (offset[0] + x_sway)
            + up * (offset[1] - y_sway - switch_drop)
            + forward * (offset[2] - self.recoil_visual);
        Mat4::from_cols(
            (right * VIEWMODEL_SCALE).extend(0.0),
            (up * VIEWMODEL_SCALE).extend(0.0),
            (forward * VIEWMODEL_SCALE).extend(0.0),
            origin.extend(1.0),
        )
    }

    pub fn animate(&mut self, dt: f32) {
        let dt = dt.max(0.0);
        let follow_alpha = 1.0 - (-CAMERA_FOLLOW_RATE * dt).exp();
        self.visual_position = self.visual_position.lerp(self.position, follow_alpha);

        let recoil_alpha = 1.0 - (-self.recoil_snappiness * dt).exp();
        self.recoil_pitch += (self.recoil_pitch_target - self.recoil_pitch) * recoil_alpha;
        self.recoil_yaw += (self.recoil_yaw_target - self.recoil_yaw) * recoil_alpha;
        self.recoil_visual += (self.recoil_visual_target - self.recoil_visual) * recoil_alpha;
        let return_alpha = (-self.recoil_return_speed * dt).exp();
        self.recoil_pitch_target *= return_alpha;
        self.recoil_yaw_target *= return_alpha;
        self.recoil_visual_target *= return_alpha;

        if self.run_amount != self.run_target {
            self.run_elapsed += dt;
            let duration = if self.run_target > self.run_from {
                0.42
            } else {
                0.28
            };
            let t = (self.run_elapsed / duration).clamp(0.0, 1.0);
            // Ease-in while entering a sprint, ease-out when leaving it.
            let eased = if self.run_target > self.run_from {
                t * t
            } else {
                1.0 - (1.0 - t) * (1.0 - t)
            };
            self.run_amount = self.run_from + (self.run_target - self.run_from) * eased;
        }

        let speed = Vec3::new(self.velocity.x, 0.0, self.velocity.z).length();
        let target = if self.on_ground {
            (speed / RUN_SPEED).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bob_alpha = 1.0 - (-12.0 * dt).exp();
        self.bob_amount += (target - self.bob_amount) * bob_alpha;
        if self.bob_amount > 0.001 {
            let cadence = WALK_CADENCE + (RUN_CADENCE - WALK_CADENCE) * self.run_amount;
            self.bob_phase += dt * cadence;
        }
    }
}

fn angle_delta(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
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
        let model = camera.view_model_with_offset([0.24, -0.19, 0.45], 0.0);
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

    #[test]
    fn viewmodel_sway_fades_smoothly_when_jump_starts() {
        let mut camera = Camera::new();
        camera.on_ground = true;
        camera.bob_amount = 0.8;
        camera.bob_phase = 0.6;
        camera.velocity.x = WALK_SPEED;
        let before_jump = camera
            .view_model_with_offset([0.24, -0.19, 0.45], 0.0)
            .w_axis
            .truncate();

        camera.on_ground = false;
        camera.animate(1.0 / 60.0);
        let after_takeoff = camera
            .view_model_with_offset([0.24, -0.19, 0.45], 0.0)
            .w_axis
            .truncate();
        assert!((after_takeoff - before_jump).length() < 0.01);
        assert!(
            camera.bob_amount > 0.0,
            "weapon bob should fade, not snap off"
        );
    }

    #[test]
    fn sprint_fov_eases_to_twelve_degree_bonus() {
        let mut camera = Camera::new();
        camera.set_running(true);
        camera.animate(0.2);
        assert!(camera.fov_degrees() > BASE_FOV);
        assert!(camera.fov_degrees() < BASE_FOV + RUN_FOV_BONUS);
        camera.animate(0.3);
        assert!((camera.fov_degrees() - (BASE_FOV + RUN_FOV_BONUS)).abs() < 1e-5);
    }

    #[test]
    fn mouse_motion_is_scaled_but_applied_immediately() {
        let mut camera = Camera::new();
        let start_yaw = camera.yaw;
        camera.apply_mouse_motion(100.0, 0.0, 0.5);
        let expected_yaw = start_yaw + 0.1;
        assert!((camera.yaw - expected_yaw).abs() < 1e-5);
        camera.animate(0.01);
        assert!((camera.yaw - expected_yaw).abs() < 1e-5);
    }

    #[test]
    fn weapon_recoil_kicks_camera_and_returns_toward_aim() {
        let mut camera = Camera::new();
        camera.apply_weapon_recoil(6.0, 1.0, 0.2, 40.0, 5.0);
        camera.animate(0.05);
        let kick = camera.pitch_angle();
        assert!(kick > 0.0);
        assert!(camera.recoil_visual > 0.0);
        for _ in 0..120 {
            camera.animate(1.0 / 60.0);
        }
        assert!(camera.pitch_angle() < kick);
        assert!(camera.recoil_visual < 0.01);
    }
}
