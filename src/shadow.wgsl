struct ShadowUniforms {
    light_view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> shadow: ShadowUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
};

@vertex
fn vs_shadow(input: VertexInput) -> @builtin(position) vec4<f32> {
    return shadow.light_view_projection * vec4<f32>(input.position, 1.0);
}

@fragment
fn fs_shadow(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let depth = position.z;
    let dx = dpdx(depth);
    let dy = dpdy(depth);
    let second_moment = depth * depth + 0.25 * (dx * dx + dy * dy);
    return vec4<f32>(depth, second_moment, 0.0, 1.0);
}
