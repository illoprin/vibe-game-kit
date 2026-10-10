use std::{error::Error, fs, path::Path};

use glam::{Mat4, Vec3};
use serde_json::Value;

use crate::{
    map::MaterialDef,
    mesh::add_box_materials_with_uv,
    obj,
    types::{Mesh, Vertex},
};

pub fn load_model(
    path: &Path,
    default_material: u32,
    materials: &[MaterialDef],
) -> Result<Mesh, Box<dyn Error>> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "obj" => obj::load_obj(path, default_material),
        "mjson" | "json" => load_mjson(path, default_material, materials),
        ext => Err(format!("unsupported model format .{ext}: {}", path.display()).into()),
    }
}

pub fn place_model(mesh: &mut Mesh, position: [f32; 3], scale: f32) {
    mesh.transform(Mat4::from_scale_rotation_translation(
        Vec3::splat(scale),
        glam::Quat::IDENTITY,
        Vec3::from_array(position),
    ));
}

fn load_mjson(
    path: &Path,
    default_material: u32,
    materials: &[MaterialDef],
) -> Result<Mesh, Box<dyn Error>> {
    let root: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let textures = root
        .get("textures")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut mesh = Mesh::default();
    let elements = root
        .get("elements")
        .and_then(Value::as_array)
        .ok_or("mjson requires an elements array")?;
    for element in elements {
        let from = vec3(element.get("from").ok_or("element missing from")?)?;
        let to = vec3(element.get("to").ok_or("element missing to")?)?;
        let center = Vec3::new(
            (from[0] + to[0]) / 32.0 - 0.5,
            (from[1] + to[1]) / 32.0 - 0.5,
            0.5 - (from[2] + to[2]) / 32.0,
        );
        let size = Vec3::new(
            (to[0] - from[0]) / 16.0,
            (to[1] - from[1]) / 16.0,
            (to[2] - from[2]) / 16.0,
        );
        let faces = element.get("faces").and_then(Value::as_object);
        let face_materials = ["north", "south", "east", "west", "up", "down"].map(|side| {
            faces
                .and_then(|faces| faces.get(side))
                .and_then(|face| face.get("texture"))
                .and_then(Value::as_str)
                .and_then(|texture| resolve_texture(texture, &textures, materials))
                .unwrap_or(default_material)
        });
        let face_uvs = ["north", "south", "east", "west", "up", "down"].map(|side| {
            faces
                .and_then(|faces| faces.get(side))
                .and_then(|face| face.get("uv"))
                .and_then(Value::as_array)
                .filter(|uv| uv.len() == 4)
                .and_then(|uv| {
                    Some([
                        uv[0].as_f64()? as f32 / 16.0,
                        uv[1].as_f64()? as f32 / 16.0,
                        uv[2].as_f64()? as f32 / 16.0,
                        uv[3].as_f64()? as f32 / 16.0,
                    ])
                })
        });
        let start = mesh.vertices.len();
        add_box_materials_with_uv(&mut mesh, center, size, face_materials, face_uvs);
        apply_element_rotation(element, &mut mesh.vertices[start..])?;
    }
    Ok(mesh)
}

fn apply_element_rotation(element: &Value, vertices: &mut [Vertex]) -> Result<(), Box<dyn Error>> {
    let Some(rotation) = element.get("rotation") else {
        return Ok(());
    };
    let raw_origin = vec3(rotation.get("origin").ok_or("rotation missing origin")?)?;
    let origin = Vec3::new(
        raw_origin[0] / 16.0 - 0.5,
        raw_origin[1] / 16.0 - 0.5,
        0.5 - raw_origin[2] / 16.0,
    );
    let axis = match rotation
        .get("axis")
        .and_then(Value::as_str)
        .ok_or("rotation missing axis")?
    {
        "x" => Vec3::X,
        "y" => Vec3::Y,
        "z" => Vec3::Z,
        _ => return Err("rotation axis must be x, y, or z".into()),
    };
    let angle = rotation
        .get("angle")
        .and_then(Value::as_f64)
        .ok_or("rotation missing angle")? as f32;
    let turn = glam::Quat::from_axis_angle(axis, angle.to_radians());
    for vertex in vertices {
        let point = Vec3::from_array(vertex.position);
        vertex.position = (origin + turn * (point - origin)).to_array();
        vertex.normal = (turn * Vec3::from_array(vertex.normal))
            .normalize()
            .to_array();
    }
    Ok(())
}

fn resolve_texture(
    texture: &str,
    textures: &serde_json::Map<String, Value>,
    materials: &[MaterialDef],
) -> Option<u32> {
    let key = texture.strip_prefix('#').unwrap_or(texture);
    let declared = textures.get(key).and_then(Value::as_str).unwrap_or(key);
    let tail = declared.rsplit(['/', '\\', ':']).next().unwrap_or(declared);
    let stem = Path::new(tail)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(tail);
    materials
        .iter()
        .position(|m| m.name == stem)
        .map(|i| i as u32)
}

fn vec3(value: &Value) -> Result<[f32; 3], Box<dyn Error>> {
    let array = value.as_array().ok_or("expected a three-number array")?;
    if array.len() != 3 {
        return Err("expected a three-number array".into());
    }
    Ok([
        array[0].as_f64().ok_or("invalid coordinate")? as f32,
        array[1].as_f64().ok_or("invalid coordinate")? as f32,
        array[2].as_f64().ok_or("invalid coordinate")? as f32,
    ])
}
