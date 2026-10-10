use std::{collections::HashMap, error::Error};

use glam::Vec3;

use crate::{
    game::GameConfig,
    map::{Map, MapObject},
    mesh::{add_box, add_floor},
    model::{load_model, place_model},
    types::{Aabb, Mesh},
};

pub struct BuiltScene {
    pub world: Mesh,
    pub weapon_models: HashMap<String, Mesh>,
    pub pickup_models: HashMap<String, Mesh>,
    pub colliders: Vec<Aabb>,
    pub debug_material: u32,
}

pub fn build(map: &Map, game: &GameConfig) -> Result<BuiltScene, Box<dyn Error>> {
    let mut world = Mesh::default();
    let mut colliders = Vec::new();
    add_floor(
        &mut world,
        map.floor_size,
        map.material_id(&map.floor_material)?,
    );
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
                let mut model = load_model(path, map.material_id(material)?, &map.materials)?;
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
        Some(load_model(
            &map.viewmodel,
            map.material_id(&map.viewmodel_material)?,
            &map.materials,
        )?)
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
            load_model(
                &weapon.model_path,
                map.material_id(&map.floor_material)?,
                &map.materials,
            )?
        };
        weapon_models.insert(key.clone(), model);
        pickup_models.insert(
            format!("weapon:{key}"),
            load_model(
                &weapon.model_path,
                map.material_id(&map.floor_material)?,
                &map.materials,
            )?,
        );
        pickup_models.insert(
            format!("ammo:{key}"),
            load_model(
                &weapon.ammunition.model_path,
                map.material_id(&map.floor_material)?,
                &map.materials,
            )?,
        );
    }
    pickup_models.insert(
        "item:aid".into(),
        load_model(
            &game.pickups.aid_model,
            map.material_id(&map.floor_material)?,
            &map.materials,
        )?,
    );
    pickup_models.insert(
        "item:armor".into(),
        load_model(
            &game.pickups.armor_model,
            map.material_id(&map.floor_material)?,
            &map.materials,
        )?,
    );
    Ok(BuiltScene {
        world,
        weapon_models,
        pickup_models,
        colliders,
        debug_material: map.material_id("debug_red")?,
    })
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
        let mut map = Map::load("assets/map1.map", &game.name).expect("map should parse");
        assert_eq!(map.floor_size, 30.0);
        assert_eq!(map.materials.len(), 5);
        for path in game.model_paths() {
            crate::model::register_adjacent_textures(&path, &mut map.materials)
                .expect("adjacent model textures should register");
        }
        map.materials.push(crate::map::MaterialDef {
            name: "debug_red".into(),
            texture: "assets/debug_red.ppm".into(),
        });
        let built = build(&map, &game).expect("all map models should load");
        assert_eq!(built.colliders.len(), 19);
        assert!(!built.world.indices.is_empty());
        assert_eq!(built.weapon_models["carbine"].vertices.len(), 17 * 24);
    }
}
