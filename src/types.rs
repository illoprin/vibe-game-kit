use glam::{Mat4, Vec3};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub material: u32,
}

impl Vertex {
    pub const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Uint32];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub view_projection: [[f32; 4]; 4],
    pub inverse_view_projection: [[f32; 4]; 4],
    pub light_view_projection: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub view_model: [[f32; 4]; 4],
    pub camera_position: [f32; 4],
    pub sun_direction: [f32; 4],
    pub sun_color_intensity: [f32; 4],
    pub point_positions_radius: [[f32; 4]; 2],
    pub point_colors_intensity: [[f32; 4]; 2],
    pub ssao_params: [f32; 4],   // intensity, radius, enabled, depth bias
    pub shadow_params: [f32; 4], // filter mode, minimum variance, bleed reduction, depth bias
    /// Vignette strength, ACES exposure, padding.
    pub effects: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn from_center_size(center: Vec3, size: Vec3) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }
}

#[derive(Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn append(&mut self, mut other: Mesh) {
        let base = self.vertices.len() as u32;
        self.vertices.append(&mut other.vertices);
        self.indices
            .extend(other.indices.into_iter().map(|i| i + base));
    }

    pub fn transform(&mut self, transform: Mat4) {
        for vertex in &mut self.vertices {
            vertex.position = transform
                .transform_point3(Vec3::from_array(vertex.position))
                .to_array();
            vertex.normal = transform
                .transform_vector3(Vec3::from_array(vertex.normal))
                .normalize()
                .to_array();
        }
    }

    /// Flips each triangle's winding order so that faces culled as back faces
    /// become front faces (and vice versa). Needed when a mesh is rendered with
    /// a mirrored transform, such as the viewmodel basis in `camera::Camera`.
    pub fn reverse_winding(&mut self) {
        for triangle in self.indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_winding_flips_every_triangle() {
        let mut mesh = Mesh {
            vertices: Vec::new(),
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6],
        };
        mesh.reverse_winding();
        assert_eq!(mesh.indices, vec![0, 2, 1, 0, 3, 2, 4, 6, 5]);
    }
}
