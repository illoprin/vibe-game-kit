use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct MaterialDef {
    pub name: String,
    pub texture: PathBuf,
}

pub enum MapObject {
    Box {
        position: [f32; 3],
        size: [f32; 3],
        material: String,
    },
    Model {
        path: PathBuf,
        position: [f32; 3],
        scale: f32,
        material: String,
    },
}

pub struct Map {
    pub materials: Vec<MaterialDef>,
    pub floor_size: f32,
    pub floor_material: String,
    pub objects: Vec<MapObject>,
    pub viewmodel: PathBuf,
    pub viewmodel_material: String,
    pub sun: SunLight,
    pub point_lights: Vec<PointLight>,
}

#[derive(Clone, Copy, Debug)]
pub struct SunLight {
    /// Normalized direction from a surface toward the sun.
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
}

impl Map {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let path = path.as_ref();
        let text = fs::read_to_string(path)?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut map = Self {
            materials: Vec::new(),
            floor_size: 30.0,
            floor_material: "concrete".into(),
            objects: Vec::new(),
            viewmodel: PathBuf::new(),
            viewmodel_material: "crate".into(),
            sun: SunLight {
                direction: [0.35, 0.8, -0.4],
                color: [1.0, 0.92, 0.78],
                intensity: 2.5,
            },
            point_lights: Vec::new(),
        };
        for (line_number, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with("MAP") {
                continue;
            }
            let fields: Vec<_> = line.split_whitespace().collect();
            let invalid = || {
                format!(
                    "{}:{}: invalid map directive: {line}",
                    path.display(),
                    line_number + 1
                )
            };
            match fields.first().copied().unwrap_or("") {
                "material" if fields.len() == 3 => map.materials.push(MaterialDef {
                    name: fields[1].to_string(),
                    texture: resolve(base, fields[2]),
                }),
                "floor" if fields.len() == 3 => {
                    map.floor_size = fields[1].parse()?;
                    map.floor_material = fields[2].to_string();
                }
                "box" if fields.len() == 8 => map.objects.push(MapObject::Box {
                    position: [fields[1].parse()?, fields[2].parse()?, fields[3].parse()?],
                    size: [fields[4].parse()?, fields[5].parse()?, fields[6].parse()?],
                    material: fields[7].to_string(),
                }),
                "model" if fields.len() == 7 => map.objects.push(MapObject::Model {
                    path: resolve(base, fields[1]),
                    position: [fields[2].parse()?, fields[3].parse()?, fields[4].parse()?],
                    scale: fields[5].parse()?,
                    material: fields[6].to_string(),
                }),
                "viewmodel" if fields.len() == 3 => {
                    map.viewmodel = resolve(base, fields[1]);
                    map.viewmodel_material = fields[2].to_string();
                }
                "sun" if fields.len() == 8 => {
                    let direction =
                        normalize3([fields[1].parse()?, fields[2].parse()?, fields[3].parse()?])?;
                    map.sun = SunLight {
                        direction,
                        color: [fields[4].parse()?, fields[5].parse()?, fields[6].parse()?],
                        intensity: fields[7].parse()?,
                    };
                }
                "point" if fields.len() == 9 => {
                    if map.point_lights.len() == 2 {
                        return Err(format!(
                            "{}:{}: at most two point lights are supported",
                            path.display(),
                            line_number + 1
                        )
                        .into());
                    }
                    map.point_lights.push(PointLight {
                        position: [fields[1].parse()?, fields[2].parse()?, fields[3].parse()?],
                        color: [fields[4].parse()?, fields[5].parse()?, fields[6].parse()?],
                        intensity: fields[7].parse()?,
                        radius: fields[8].parse()?,
                    });
                }
                _ => return Err(invalid().into()),
            }
        }
        if map.materials.is_empty() {
            return Err("map must declare at least one material".into());
        }
        Ok(map)
    }

    pub fn material_id(&self, name: &str) -> Result<u32, Box<dyn Error>> {
        self.materials
            .iter()
            .position(|m| m.name == name)
            .map(|i| i as u32)
            .ok_or_else(|| format!("material '{name}' is not declared").into())
    }
}

fn resolve(base: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn normalize3(value: [f32; 3]) -> Result<[f32; 3], Box<dyn Error>> {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        return Err("sun direction must be a non-zero finite vector".into());
    }
    Ok([value[0] / length, value[1] / length, value[2] / length])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_map_declares_sun_and_two_point_lights() {
        let map = Map::load("assets/map1.map").expect("demo map should load");
        assert_eq!(map.point_lights.len(), 2);
        let length = map.sun.direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-5);
    }

    #[test]
    fn sun_direction_must_be_nonzero() {
        assert!(normalize3([0.0, 0.0, 0.0]).is_err());
    }
}
