use glam::Vec3;

use crate::types::{Mesh, Vertex};

pub fn add_floor(mesh: &mut Mesh, size: f32, material: u32) {
    let h = size * 0.5;
    add_quad(
        mesh,
        [[-h, 0.0, h], [h, 0.0, h], [h, 0.0, -h], [-h, 0.0, -h]],
        [0.0, 1.0, 0.0],
        [[0.0, 0.0], [size, 0.0], [size, size], [0.0, size]],
        material,
    );
}

pub fn add_box(mesh: &mut Mesh, center: Vec3, size: Vec3, material: u32) {
    add_box_materials(mesh, center, size, [material; 6]);
}

pub fn add_box_materials(mesh: &mut Mesh, center: Vec3, size: Vec3, materials: [u32; 6]) {
    add_box_materials_with_uv(mesh, center, size, materials, [None; 6]);
}

pub fn add_box_materials_with_uv(
    mesh: &mut Mesh,
    center: Vec3,
    size: Vec3,
    materials: [u32; 6],
    uv_rects: [Option<[f32; 4]>; 6],
) {
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [1.0, -1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [1.0, -1.0, 1.0],
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-1.0, -1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [-1.0, 1.0, 1.0],
                [-1.0, 1.0, -1.0],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, -1.0],
                [-1.0, 1.0, -1.0],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, -1.0, 1.0],
                [-1.0, -1.0, 1.0],
            ],
        ),
    ];
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let half = size * 0.5;
    for (face_index, (normal, corners)) in faces.iter().enumerate() {
        let (repeat_u, repeat_v) = if face_index < 2 {
            (size.x, size.y)
        } else if face_index < 4 {
            (size.z, size.y)
        } else {
            (size.x, size.z)
        };
        let converted = std::array::from_fn(|i| {
            let point = center + Vec3::from_array(corners[i]) * half;
            let face_uv = if let Some([u1, v1, u2, v2]) = uv_rects[face_index] {
                [[u1, v2], [u2, v2], [u2, v1], [u1, v1]][i]
            } else {
                [uv[i][0] * repeat_u, uv[i][1] * repeat_v]
            };
            Vertex {
                position: point.to_array(),
                normal: *normal,
                uv: face_uv,
                material: materials[face_index],
            }
        });
        add_face(mesh, converted);
    }
}

pub fn add_quad(
    mesh: &mut Mesh,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    uv: [[f32; 2]; 4],
    material: u32,
) {
    let vertices = std::array::from_fn(|i| Vertex {
        position: corners[i],
        normal,
        uv: uv[i],
        material,
    });
    add_face(mesh, vertices);
}

fn add_face(mesh: &mut Mesh, vertices: [Vertex; 4]) {
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend(vertices);
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}
