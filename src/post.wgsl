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
@group(0) @binding(1) var hdr_texture: texture_2d<f32>;

struct FullscreenOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    var output: FullscreenOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    return output;
}

fn aces_fitted(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_post(input: FullscreenOutput) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(hdr_texture));
    let pixel = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), size - vec2<i32>(1));
    var color = textureLoad(hdr_texture, pixel, 0).rgb * uniforms.effects.y;
    let uv = (vec2<f32>(pixel) + vec2<f32>(0.5)) / vec2<f32>(size);
    let centered = (uv - vec2<f32>(0.5)) * vec2<f32>(1.0, f32(size.y) / f32(size.x));
    let vignette = 1.0 - uniforms.effects.x * smoothstep(0.2, 0.72, length(centered));
    color = aces_fitted(color * vignette);
    return vec4<f32>(color, 1.0);
}
