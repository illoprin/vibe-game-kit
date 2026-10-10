use std::error::Error;

use glam::Vec3;

use crate::{
    map::{Map, MapObject},
    mesh::{add_box, add_floor},
    model::{load_model, place_model},
    types::{Aabb, Mesh},
};

pub struct BuiltScene {
    pub world: Mesh,
    pub viewmodel: Mesh,
    pub colliders: Vec<Aabb>,
}

pub fn build(map: &Map) -> Result<BuiltScene, Box<dyn Error>> {
    let mut world = Mesh::default();
    let mut colliders = Vec::new();
    add_floor(
        &mut world,
        map.floor_size,
        map.material_id(&map.floor_material)?,
    );
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
    let viewmodel = load_model(
        &map.viewmodel,
        map.material_id(&map.viewmodel_material)?,
        &map.materials,
    )?;
    Ok(BuiltScene {
        world,
        viewmodel,
        colliders,
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
        let map = Map::load("assets/map1.map").expect("map should parse");
        assert_eq!(map.floor_size, 30.0);
        assert_eq!(map.materials.len(), 5);
        let built = build(&map).expect("all map models should load");
        assert_eq!(built.colliders.len(), 18);
        assert!(!built.world.indices.is_empty());
        assert_eq!(built.viewmodel.vertices.len(), 17 * 24);
    }
}
