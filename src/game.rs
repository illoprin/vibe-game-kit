use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    camera::Camera,
    collision,
    map::{Map, MapPickup},
    rigidbody::RigidBody,
    types::Aabb,
};
use glam::{Mat4, Vec3};
use serde::Deserialize;

pub const DEFAULT_GAME_FILE: &str = "game.toml";
const PICKUP_SCALE: f32 = 0.9;
const PICKUP_HALF_EXTENTS: f32 = PICKUP_SCALE * 0.5;
const IMPACT_SIZE: f32 = 1.0 / 16.0;

#[derive(Clone, Debug, Deserialize)]
pub struct GameConfig {
    pub name: String,
    #[serde(default)]
    pub pickups: PickupModels,
    pub weapons: BTreeMap<String, WeaponConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PickupModels {
    pub aid_model: PathBuf,
    pub armor_model: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WeaponConfig {
    pub name: String,
    pub model_path: PathBuf,
    #[serde(default = "default_fpv_offset")]
    pub fpv_offset: [f32; 3],
    #[serde(default = "one")]
    pub bullets_per_shot: u32,
    #[serde(default = "default_fire_rate")]
    pub fire_rate: f32,
    #[serde(default)]
    pub max_spread: f32,
    pub ammunition: AmmunitionConfig,
    #[serde(default)]
    pub recoil: RecoilConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AmmunitionConfig {
    pub model_path: PathBuf,
    pub amount: u32,
    pub max: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct RecoilConfig {
    pub pitch_kick: f32,
    pub yaw_kick_min: f32,
    pub yaw_kick_max: f32,
    pub visual_kickback: f32,
    pub snappiness: f32,
    pub return_speed: f32,
}

impl Default for RecoilConfig {
    fn default() -> Self {
        Self {
            pitch_kick: 2.0,
            yaw_kick_min: -0.4,
            yaw_kick_max: 0.4,
            visual_kickback: 0.12,
            snappiness: 25.0,
            return_speed: 8.0,
        }
    }
}

impl GameConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&text)?;
        if !valid_game_name(&config.name) {
            return Err("game.name must be non-empty and contain no whitespace".into());
        }
        if config.weapons.is_empty() {
            return Err("game.toml must declare at least one weapon".into());
        }
        if config.weapons.len() > 9 {
            return Err("at most nine weapons are supported (number keys 1–9)".into());
        }

        let base = path.parent().unwrap_or_else(|| Path::new("."));
        if config.pickups.aid_model.as_os_str().is_empty()
            || config.pickups.armor_model.as_os_str().is_empty()
        {
            return Err("[pickups] must define both aid_model and armor_model".into());
        }
        resolve_if_relative(base, &mut config.pickups.aid_model);
        resolve_if_relative(base, &mut config.pickups.armor_model);
        for (key, weapon) in &mut config.weapons {
            if key.is_empty() || key.chars().any(char::is_whitespace) {
                return Err(format!("invalid weapon key '{key}'").into());
            }
            if weapon.name.trim().is_empty()
                || weapon.model_path.as_os_str().is_empty()
                || weapon.ammunition.model_path.as_os_str().is_empty()
            {
                return Err(format!("weapon '{key}' is missing its name or model path").into());
            }
            resolve_if_relative(base, &mut weapon.model_path);
            resolve_if_relative(base, &mut weapon.ammunition.model_path);
            if weapon.bullets_per_shot == 0
                || !weapon.fire_rate.is_finite()
                || weapon.fire_rate <= 0.0
                || !weapon.max_spread.is_finite()
                || weapon.max_spread < 0.0
                || !weapon.fpv_offset.iter().all(|value| value.is_finite())
                || weapon.ammunition.max == 0
                || weapon.ammunition.amount == 0
                || weapon.ammunition.amount > weapon.ammunition.max
            {
                return Err(
                    format!("weapon '{key}' has invalid firing or ammunition parameters").into(),
                );
            }
            let recoil = &weapon.recoil;
            if ![
                recoil.pitch_kick,
                recoil.yaw_kick_min,
                recoil.yaw_kick_max,
                recoil.visual_kickback,
                recoil.snappiness,
                recoil.return_speed,
            ]
            .iter()
            .all(|value| value.is_finite())
                || recoil.snappiness <= 0.0
                || recoil.return_speed <= 0.0
                || recoil.yaw_kick_min > recoil.yaw_kick_max
            {
                return Err(format!("weapon '{key}' has invalid recoil parameters").into());
            }
        }
        Ok(config)
    }

    pub fn model_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![
            self.pickups.aid_model.clone(),
            self.pickups.armor_model.clone(),
        ];
        for weapon in self.weapons.values() {
            paths.push(weapon.model_path.clone());
            paths.push(weapon.ammunition.model_path.clone());
        }
        paths.sort();
        paths.dedup();
        paths
    }
}

fn resolve_if_relative(base: &Path, path: &mut PathBuf) {
    if !path.as_os_str().is_empty() && path.is_relative() {
        *path = base.join(&*path);
    }
}

fn default_fpv_offset() -> [f32; 3] {
    [0.24, -0.19, 0.45]
}

fn one() -> u32 {
    1
}

fn default_fire_rate() -> f32 {
    10.0
}

fn valid_game_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(char::is_whitespace)
}

#[derive(Clone, Debug)]
pub enum PickupKind {
    Weapon(String),
    Ammunition(String),
    Aid,
    Armor,
}

impl PickupKind {
    pub fn model_key(&self) -> String {
        match self {
            Self::Weapon(key) => format!("weapon:{key}"),
            Self::Ammunition(key) => format!("ammo:{key}"),
            Self::Aid => "item:aid".into(),
            Self::Armor => "item:armor".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorldPickup {
    pub kind: PickupKind,
    pub body: RigidBody,
    pub yaw: f32,
    pub active: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ImpactMark {
    pub position: Vec3,
    pub lifetime: f32,
}

pub struct GameState {
    pub pickups: Vec<WorldPickup>,
    pub impacts: Vec<ImpactMark>,
    pub weapon_keys: Vec<String>,
    pub selected_weapon: String,
    pub ammunition: BTreeMap<String, u32>,
    owned_weapons: HashSet<String>,
    pub health: u32,
    pub armor: u32,
    fire_elapsed: f32,
    switch_elapsed: f32,
    random_state: u32,
}

impl GameState {
    pub fn new(config: &GameConfig, map: &Map) -> Result<Self, Box<dyn Error>> {
        let mut pickups = Vec::with_capacity(map.pickups.len());
        for entry in &map.pickups {
            let kind = match entry {
                MapPickup::Weapon { key, .. } => {
                    if !config.weapons.contains_key(key) {
                        return Err(format!("map references unknown weapon '{key}'").into());
                    }
                    PickupKind::Weapon(key.clone())
                }
                MapPickup::Ammunition { key, .. } => {
                    if !config.weapons.contains_key(key) {
                        return Err(format!(
                            "map references ammunition for unknown weapon '{key}'"
                        )
                        .into());
                    }
                    PickupKind::Ammunition(key.clone())
                }
                MapPickup::Item { kind, .. } => match kind.as_str() {
                    "aid" => PickupKind::Aid,
                    "armor" => PickupKind::Armor,
                    _ => return Err(format!("unknown item pickup type '{kind}'").into()),
                },
            };
            let position = match entry {
                MapPickup::Weapon { position, .. }
                | MapPickup::Ammunition { position, .. }
                | MapPickup::Item { position, .. } => Vec3::from_array(*position),
            };
            pickups.push(WorldPickup {
                kind,
                body: RigidBody::new(position, Vec3::splat(PICKUP_HALF_EXTENTS)),
                yaw: 0.0,
                active: true,
            });
        }

        let weapon_keys = config.weapons.keys().cloned().collect::<Vec<_>>();
        let selected_weapon = weapon_keys
            .first()
            .cloned()
            .ok_or("game.toml must declare at least one weapon")?;
        let mut ammunition = BTreeMap::new();
        for key in &weapon_keys {
            ammunition.insert(key.clone(), 0);
        }
        ammunition.insert(
            selected_weapon.clone(),
            config.weapons[&selected_weapon].ammunition.max,
        );

        Ok(Self {
            pickups,
            impacts: Vec::new(),
            owned_weapons: HashSet::from([selected_weapon.clone()]),
            weapon_keys,
            selected_weapon,
            ammunition,
            health: 100,
            armor: 0,
            fire_elapsed: 0.0,
            switch_elapsed: 0.35,
            random_state: 0x6d2b_79f5,
        })
    }

    pub fn select_slot(&mut self, slot: usize) -> bool {
        let Some(key) = self.weapon_keys.get(slot).cloned() else {
            return false;
        };
        self.select_weapon(&key)
    }

    pub fn cycle_weapon(&mut self, direction: i32) {
        let mut available = self
            .weapon_keys
            .iter()
            .filter(|key| self.owned_weapons.contains(*key));
        let owned = available.by_ref().count();
        if owned < 2 {
            return;
        }
        let current = self
            .weapon_keys
            .iter()
            .position(|key| key == &self.selected_weapon)
            .unwrap_or(0);
        let step = if direction >= 0 { 1isize } else { -1isize };
        for offset in 1..=self.weapon_keys.len() {
            let candidate = (current as isize + step * offset as isize)
                .rem_euclid(self.weapon_keys.len() as isize) as usize;
            if self.owned_weapons.contains(&self.weapon_keys[candidate]) {
                self.select_weapon(&self.weapon_keys[candidate].clone());
                break;
            }
        }
    }

    fn select_weapon(&mut self, key: &str) -> bool {
        if key == self.selected_weapon || !self.owned_weapons.contains(key) {
            return false;
        }
        self.selected_weapon = key.to_string();
        self.switch_elapsed = 0.0;
        self.fire_elapsed = 0.0;
        true
    }

    pub fn switch_drop(&self) -> f32 {
        let t = (self.switch_elapsed / 0.35).clamp(0.0, 1.0);
        0.32 * (1.0 - t) * (1.0 - t)
    }

    pub fn update(
        &mut self,
        dt: f32,
        camera: &mut Camera,
        config: &GameConfig,
        firing: bool,
        static_colliders: &[Aabb],
    ) {
        let dt = dt.clamp(0.0, 0.05);
        self.switch_elapsed = (self.switch_elapsed + dt).min(0.35);
        self.fire_elapsed += dt;
        for impact in &mut self.impacts {
            impact.lifetime -= dt;
        }
        self.impacts.retain(|impact| impact.lifetime > 0.0);

        for pickup in &mut self.pickups {
            if !pickup.active {
                continue;
            }
            collision::update_rigidbody(&mut pickup.body, static_colliders, dt);
            pickup.yaw =
                (pickup.yaw + std::f32::consts::PI / 6.0 * dt).rem_euclid(std::f32::consts::TAU);
        }
        self.collect_nearby(camera, config);

        let mut aim_colliders = static_colliders.to_vec();
        aim_colliders.extend(
            self.pickups
                .iter()
                .filter(|pickup| pickup.active)
                .map(|pickup| pickup.body.collider()),
        );

        while firing {
            let weapon = &config.weapons[&self.selected_weapon];
            let interval = 1.0 / weapon.fire_rate;
            if self.fire_elapsed < interval {
                break;
            }
            if self
                .ammunition
                .get(&self.selected_weapon)
                .copied()
                .unwrap_or(0)
                == 0
            {
                self.fire_elapsed = interval;
                break;
            }
            self.fire_elapsed -= interval;
            *self.ammunition.get_mut(&self.selected_weapon).unwrap() -= 1;
            self.fire_weapon(camera, weapon, &aim_colliders);
        }
        if !firing {
            self.fire_elapsed = self
                .fire_elapsed
                .min(1.0 / config.weapons[&self.selected_weapon].fire_rate);
        }
    }

    fn collect_nearby(&mut self, camera: &Camera, config: &GameConfig) {
        let player_center = camera.position + Vec3::Y * 0.9;
        for pickup in &mut self.pickups {
            if !pickup.active || pickup.body.position.distance(player_center) > 1.25 {
                continue;
            }
            match &pickup.kind {
                PickupKind::Weapon(key) => {
                    self.owned_weapons.insert(key.clone());
                    let ammo = &config.weapons[key].ammunition;
                    let current = self.ammunition.entry(key.clone()).or_default();
                    *current = current.saturating_add(ammo.amount).min(ammo.max);
                    self.selected_weapon = key.clone();
                    self.switch_elapsed = 0.0;
                    self.fire_elapsed = 0.0;
                }
                PickupKind::Ammunition(key) => {
                    let ammo = &config.weapons[key].ammunition;
                    let current = self.ammunition.entry(key.clone()).or_default();
                    *current = current.saturating_add(ammo.amount).min(ammo.max);
                }
                PickupKind::Aid => self.health = self.health.saturating_add(50).min(100),
                PickupKind::Armor => self.armor = self.armor.saturating_add(50).min(100),
            }
            pickup.active = false;
        }
    }

    fn fire_weapon(&mut self, camera: &mut Camera, weapon: &WeaponConfig, colliders: &[Aabb]) {
        let forward = camera.forward();
        let right = camera.right();
        let up = right.cross(forward).normalize_or_zero();
        let origin = camera.position + Vec3::Y * 1.62;
        for _ in 0..weapon.bullets_per_shot {
            let spread_x = self.random_range(-weapon.max_spread, weapon.max_spread);
            let spread_y = self.random_range(-weapon.max_spread, weapon.max_spread);
            let direction = (forward + right * spread_x + up * spread_y).normalize_or_zero();
            if let Some(distance) = colliders
                .iter()
                .filter_map(|collider| ray_aabb(origin, direction, *collider))
                .filter(|distance| *distance >= 0.0 && *distance <= 100.0)
                .min_by(f32::total_cmp)
            {
                if self.impacts.len() >= 256 {
                    self.impacts.remove(0);
                }
                self.impacts.push(ImpactMark {
                    position: origin + direction * distance,
                    lifetime: 20.0,
                });
            }
        }
        let yaw = self.random_range(weapon.recoil.yaw_kick_min, weapon.recoil.yaw_kick_max);
        camera.apply_weapon_recoil(
            weapon.recoil.pitch_kick,
            yaw,
            weapon.recoil.visual_kickback,
            weapon.recoil.snappiness,
            weapon.recoil.return_speed,
        );
    }

    fn random_range(&mut self, min: f32, max: f32) -> f32 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 17;
        self.random_state ^= self.random_state << 5;
        let unit = self.random_state as f32 / u32::MAX as f32;
        min + (max - min) * unit
    }

    pub fn selected_ammunition(&self) -> u32 {
        self.ammunition
            .get(&self.selected_weapon)
            .copied()
            .unwrap_or(0)
    }

    pub fn hud_text(&self, config: &GameConfig) -> String {
        format!(
            "{}  |  Ammo: {} / {}  |  HP: {}  Armor: {}",
            config.weapons[&self.selected_weapon].name,
            self.selected_ammunition(),
            config.weapons[&self.selected_weapon].ammunition.max,
            self.health,
            self.armor
        )
    }

    pub fn pickup_model_transform(pickup: &WorldPickup) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::splat(PICKUP_SCALE),
            glam::Quat::from_rotation_y(pickup.yaw),
            pickup.body.position,
        )
    }

    pub fn impact_size() -> f32 {
        IMPACT_SIZE
    }
}

fn ray_aabb(origin: Vec3, direction: Vec3, bounds: Aabb) -> Option<f32> {
    let mut near = 0.0f32;
    let mut far = f32::INFINITY;
    for axis in 0..3 {
        let o = origin[axis];
        let d = direction[axis];
        let min = bounds.min[axis];
        let max = bounds.max[axis];
        if d.abs() < 1e-7 {
            if o < min || o > max {
                return None;
            }
            continue;
        }
        let inverse = d.recip();
        let mut a = (min - o) * inverse;
        let mut b = (max - o) * inverse;
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        near = near.max(a);
        far = far.min(b);
        if near > far {
            return None;
        }
    }
    (far >= 0.0).then_some(near.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{map::Map, types::Aabb};

    #[test]
    fn ray_hits_nearest_aabb_face() {
        let bounds = Aabb::from_center_size(Vec3::new(0.0, 0.0, -3.0), Vec3::ONE);
        assert_eq!(ray_aabb(Vec3::ZERO, -Vec3::Z, bounds), Some(2.5));
        assert_eq!(ray_aabb(Vec3::ZERO, Vec3::X, bounds), None);
    }

    #[test]
    fn game_name_cannot_contain_spaces() {
        assert!(!valid_game_name("two words"));
        assert!(valid_game_name("blue_game1"));
        assert!(valid_game_name("blue-game"));
    }

    #[test]
    fn weapon_pickup_unlocks_weapon_and_adds_configured_ammo() {
        let config = GameConfig::load("game.toml").expect("sample game should load");
        let map = Map::load("assets/map/map1.map", &config.name).expect("sample map should load");
        let mut state = GameState::new(&config, &map).expect("pickups should validate");
        let mut camera = Camera::new();
        state.pickups[0].body.position = camera.position;
        state.update(
            0.01,
            &mut camera,
            &config,
            false,
            &[Aabb::from_center_size(
                Vec3::new(0.0, -0.05, 0.0),
                Vec3::new(30.0, 0.1, 30.0),
            )],
        );
        assert_eq!(state.selected_weapon, "shotgun");
        assert_eq!(state.ammunition["shotgun"], 4);
        assert!(!state.pickups[0].active);
    }

    #[test]
    fn held_fire_spends_one_round_and_creates_impact_cube() {
        let config = GameConfig::load("game.toml").expect("sample game should load");
        let map = Map::load("assets/map/map1.map", &config.name).expect("sample map should load");
        let mut state = GameState::new(&config, &map).expect("pickups should validate");
        let mut camera = Camera::new();
        let colliders = [Aabb::from_center_size(Vec3::new(0.0, 1.62, 0.0), Vec3::ONE)];
        for _ in 0..3 {
            state.update(0.05, &mut camera, &config, true, &colliders);
        }
        assert_eq!(
            state.selected_ammunition(),
            config.weapons["carbine"].ammunition.max - 1
        );
        assert_eq!(state.impacts.len(), 1);
        assert!((state.impacts[0].position.z - 0.5).abs() < 0.05);
        assert!((GameState::impact_size() - 1.0 / 16.0).abs() < f32::EPSILON);
    }
}
