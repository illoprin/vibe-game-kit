use std::{collections::HashMap, error::Error, path::Path};

use glam::Vec3;

use crate::{
    game::GameConfig,
    map::{Map, MapObject, MaterialDef},
    mesh::{add_box, add_floor},
    model::{load_model, place_model, placeholder_mesh},
    types::{Aabb, Mesh},
};

/// Material whose texture is deliberately absent so the texture loader falls
/// back to the magenta checkerboard. Used by the missing-model cube.
pub const MISSING_TEXTURE_NAME: &str = "missing_texture";
const MISSING_TEXTURE_PATH: &str = "assets/missing_texture.ppm";

pub struct BuiltScene {
    pub world: Mesh,
    pub weapon_models: HashMap<String, Mesh>,
    pub pickup_models: HashMap<String, Mesh>,
    pub colliders: Vec<Aabb>,
    pub debug_material: u32,
}

pub fn build(map: &mut Map, game: &GameConfig) -> Result<BuiltScene, Box<dyn Error>> {
    let missing_material = ensure_missing_material(map);
    let floor_material = map.material_id(&map.floor_material)?;
    let mut world = Mesh::default();
    let mut colliders = Vec::new();
    add_floor(&mut world, map.floor_size, floor_material);
    colliders.push(Aabb::from_center_size(
        Vec3::new(0.0, -0.05, 0.0),
        Vec3::new(map.floor_size, 0.1, map.floor_size),
    ));
    for object in &map.objects {
        match object {
            MapObject::Box {
                position,
                size,
                material,
            } => {
                let center = Vec3::from_array(*position);
                let dimensions = Vec3::from_array(*size);
                add_box(&mut world, center, dimensions, map.material_id(material)?);
                colliders.push(Aabb::from_center_size(center, dimensions));
            }
            MapObject::Model {
                path,
                position,
                scale,
                material,
            } => {
                let mut model = load_or_placeholder(
                    path,
                    map.material_id(material)?,
                    missing_material,
                    &map.materials,
                );
                place_model(&mut model, *position, *scale);
                if !model.vertices.is_empty() {
                    colliders.push(bounds(
                        &model
                            .vertices
                            .iter()
                            .map(|v| Vec3::from_array(v.position))
                            .collect::<Vec<_>>(),
                    ));
                }
                world.append(model);
            }
        }
    }
    let legacy_viewmodel = if game
        .weapons
        .values()
        .any(|weapon| weapon.model_path == map.viewmodel)
    {
        Some(load_or_placeholder(
            &map.viewmodel,
            map.material_id(&map.viewmodel_material)?,
            missing_material,
            &map.materials,
        ))
    } else {
        None
    };
    let mut weapon_models = HashMap::new();
    let mut pickup_models = HashMap::new();
    for (key, weapon) in &game.weapons {
        let model = if weapon.model_path == map.viewmodel {
            legacy_viewmodel
                .as_ref()
                .expect("matching legacy viewmodel was loaded")
                .clone()
        } else {
            load_or_placeholder(
                &weapon.model_path,
                floor_material,
                missing_material,
                &map.materials,
            )
        };
        weapon_models.insert(key.clone(), model);
        pickup_models.insert(
            format!("weapon:{key}"),
            load_or_placeholder(
                &weapon.model_path,
                floor_material,
                missing_material,
                &map.materials,
            ),
        );
        pickup_models.insert(
            format!("ammo:{key}"),
            load_or_placeholder(
                &weapon.ammunition.model_path,
                floor_material,
                missing_material,
                &map.materials,
            ),
        );
    }
    pickup_models.insert(
        "item:aid".into(),
        load_or_placeholder(
            &game.pickups.aid_model,
            floor_material,
            missing_material,
            &map.materials,
        ),
    );
    pickup_models.insert(
        "item:armor".into(),
        load_or_placeholder(
            &game.pickups.armor_model,
            floor_material,
            missing_material,
            &map.materials,
        ),
    );
    Ok(BuiltScene {
        world,
        weapon_models,
        pickup_models,
        colliders,
        debug_material: map.material_id("debug_red")?,
    })
}

/// Makes sure the checkerboard material is present and returns its index.
fn ensure_missing_material(map: &mut Map) -> u32 {
    if let Some(index) = map
        .materials
        .iter()
        .position(|material| material.name == MISSING_TEXTURE_NAME)
    {
        return index as u32;
    }
    map.materials.push(MaterialDef {
        name: MISSING_TEXTURE_NAME.into(),
        texture: MISSING_TEXTURE_PATH.into(),
    });
    (map.materials.len() - 1) as u32
}

/// Loads a model, falling back to the checkerboard cube when the file is
/// missing or cannot be parsed so a broken asset never aborts the scene.
fn load_or_placeholder(
    path: &Path,
    default_material: u32,
    missing_material: u32,
    materials: &[MaterialDef],
) -> Mesh {
    match load_model(path, default_material, materials) {
        Ok(mesh) => mesh,
        Err(error) => {
            eprintln!(
                "could not load model {} ({error}); using the missing-model checker cube",
                path.display()
            );
            placeholder_mesh(missing_material)
        }
    }
}

fn bounds(points: &[Vec3]) -> Aabb {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for point in points {
        min = min.min(*point);
        max = max.max(*point);
    }
    Aabb { min, max }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_loads_scene_and_held_minecraft_model() {
        let game = GameConfig::load("game.toml").expect("game config should parse");
        let mut map = Map::load("assets/map/map1.map", &game.name).expect("map should parse");
        assert_eq!(map.floor_size, 30.0);
        assert_eq!(map.materials.len(), 5);
        for path in game.model_paths() {
            crate::model::register_adjacent_textures(&path, &mut map.materials);
        }
        map.materials.push(crate::map::MaterialDef {
            name: "debug_red".into(),
            texture: "assets/debug_red.ppm".into(),
        });
        let built = build(&mut map, &game).expect("all map models should load");
        assert_eq!(built.colliders.len(), 19);
        assert!(!built.world.indices.is_empty());
        assert_eq!(built.weapon_models["carbine"].vertices.len(), 17 * 24);
        assert!(map.material_id(MISSING_TEXTURE_NAME).is_ok());
    }

    #[test]
    fn missing_map_model_becomes_a_checker_cube() {
        let directory =
            std::env::temp_dir().join(format!("ogk-missing-model-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temporary folder should be created");
        let map_path = directory.join("test.map");
        std::fs::write(
            &map_path,
            "MAP 1\nGAME test\nmaterial concrete concrete.ppm\nmaterial debug_red debug_red.ppm\nfloor 4 concrete\nmodel does_not_exist.json 0 2 0 1 concrete\n",
        )
        .expect("temporary map should be written");
        let game = GameConfig {
            name: "test".into(),
            pickups: crate::game::PickupModels {
                aid_model: "missing_aid.json".into(),
                armor_model: "missing_armor.json".into(),
            },
            weapons: Default::default(),
        };
        let mut map = Map::load(&map_path, "test").expect("temporary map should parse");
        let built = build(&mut map, &game).expect("missing models must not abort the build");
        // Floor collider plus the placeholder cube for the missing map model.
        assert_eq!(built.colliders.len(), 2);
        let placeholder = built
            .pickup_models
            .get("item:aid")
            .expect("a missing pickup model should still yield a mesh");
        assert_eq!(placeholder.vertices.len(), 24);
        assert_eq!(
            placeholder.vertices[0].material,
            map.material_id(MISSING_TEXTURE_NAME).unwrap()
        );
        std::fs::remove_dir_all(directory).expect("temporary folder should be removed");
    }
}
