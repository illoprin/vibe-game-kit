use std::{error::Error, fs, path::Path};

use glam::Vec3;

use crate::types::{Mesh, Vertex};

type ObjVertex = (usize, Option<usize>, Option<usize>);

pub fn load_obj(path: &Path, material: u32) -> Result<Mesh, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    let mut positions = Vec::<Vec3>::new();
    let mut texcoords = Vec::<[f32; 2]>::new();
    let mut normals = Vec::<Vec3>::new();
    let mut mesh = Mesh::default();
    for line in source.lines() {
        let mut fields = line.split_whitespace();
        match fields.next().unwrap_or("") {
            "v" => positions.push(Vec3::new(
                parse(fields.next())?,
                parse(fields.next())?,
                parse(fields.next())?,
            )),
            "vt" => texcoords.push([parse(fields.next())?, parse(fields.next())?]),
            "vn" => normals.push(
                Vec3::new(
                    parse(fields.next())?,
                    parse(fields.next())?,
                    parse(fields.next())?,
                )
                .normalize_or_zero(),
            ),
            "f" => {
                let face: Vec<_> = fields
                    .map(|token| {
                        parse_vertex(token, positions.len(), texcoords.len(), normals.len())
                    })
                    .collect::<Result<_, _>>()?;
                if face.len() < 3 {
                    return Err("OBJ face has fewer than three vertices".into());
                }
                for i in 1..face.len() - 1 {
                    push_triangle(
                        &mut mesh,
                        [face[0], face[i], face[i + 1]],
                        &positions,
                        &texcoords,
                        &normals,
                        material,
                    );
                }
            }
            _ => {}
        }
    }
    if mesh.indices.is_empty() {
        return Err(format!("OBJ has no faces: {}", path.display()).into());
    }
    Ok(mesh)
}

fn parse_vertex(
    token: &str,
    p_count: usize,
    t_count: usize,
    n_count: usize,
) -> Result<ObjVertex, Box<dyn Error>> {
    let parts: Vec<_> = token.split('/').collect();
    Ok((
        index(parts.first().copied().unwrap_or(""), p_count)?,
        parts
            .get(1)
            .filter(|v| !v.is_empty())
            .map(|v| index(v, t_count))
            .transpose()?,
        parts
            .get(2)
            .filter(|v| !v.is_empty())
            .map(|v| index(v, n_count))
            .transpose()?,
    ))
}

fn index(token: &str, count: usize) -> Result<usize, Box<dyn Error>> {
    let parsed: isize = token.parse()?;
    let resolved = if parsed < 0 {
        count as isize + parsed
    } else {
        parsed - 1
    };
    if resolved < 0 || resolved as usize >= count {
        return Err("OBJ index out of range".into());
    }
    Ok(resolved as usize)
}

fn push_triangle(
    mesh: &mut Mesh,
    face: [ObjVertex; 3],
    positions: &[Vec3],
    uvs: &[[f32; 2]],
    normals: &[Vec3],
    material: u32,
) {
    let normal = (positions[face[1].0] - positions[face[0].0])
        .cross(positions[face[2].0] - positions[face[0].0])
        .normalize_or_zero();
    let first = mesh.vertices.len() as u32;
    for item in face {
        mesh.vertices.push(Vertex {
            position: positions[item.0].to_array(),
            normal: item.2.map(|i| normals[i]).unwrap_or(normal).to_array(),
            uv: item.1.map(|i| uvs[i]).unwrap_or([0.0, 0.0]),
            material,
        });
    }
    mesh.indices
        .extend_from_slice(&[first, first + 1, first + 2]);
}

fn parse(value: Option<&str>) -> Result<f32, Box<dyn Error>> {
    Ok(value.ok_or("incomplete OBJ vertex")?.parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obj_faces_accept_uvs_and_negative_indices() {
        let path = std::env::temp_dir().join(format!("ogk_obj_{}.obj", std::process::id()));
        fs::write(
            &path,
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf -3/1 -2/2 -1/3\n",
        )
        .expect("write temporary OBJ");
        let mesh = load_obj(&path, 0).expect("OBJ should load");
        let _ = fs::remove_file(path);
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.indices, [0, 1, 2]);
        assert_eq!(mesh.vertices[1].uv, [1.0, 0.0]);
    }
}
