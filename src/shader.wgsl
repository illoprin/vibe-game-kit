struct Uniforms {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    light_view_projection: mat4x4<f32>,
    model: mat4x4<f32>,
    view_model: mat4x4<f32>,
    camera_position: vec4<f32>,
    sun_direction: vec4<f32>,
    sun_color_intensity: vec4<f32>,
    point_positions_radius: array<vec4<f32>, 2>,
    point_colors_intensity: array<vec4<f32>, 2>,
    ssao_params: vec4<f32>,
    shadow_params: vec4<f32>,
    effects: vec4<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var color_texture: texture_2d_array<f32>;
@group(0) @binding(2) var color_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) material: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) material: u32,
};

struct GBufferOutput {
    @location(0) albedo: vec4<f32>,
    @location(1) normal: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    var transform = uniforms.model;
    if (input.material & 0x80000000u) != 0u {
        transform = uniforms.view_model;
    }
    let world_position = transform * vec4<f32>(input.position, 1.0);
    output.clip_position = uniforms.view_projection * world_position;
    output.world_position = world_position.xyz;
    output.normal = normalize((transform * vec4<f32>(input.normal, 0.0)).xyz);
    output.uv = input.uv;
    output.material = input.material & 0x7fffffffu;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> GBufferOutput {
    var output: GBufferOutput;
    output.albedo = textureSample(color_texture, color_sampler, input.uv, i32(input.material));
    output.normal = vec4<f32>(normalize(input.normal), 1.0);
    return output;
}
