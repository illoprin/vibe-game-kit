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
@group(0) @binding(1) var raw_ssao: texture_2d<f32>;
@group(0) @binding(2) var normal_texture: texture_2d<f32>;
@group(0) @binding(3) var depth_texture: texture_depth_2d;
@group(0) @binding(4) var viewmodel_depth_texture: texture_depth_2d;

struct FullscreenOutput {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    var output: FullscreenOutput;
    output.position = vec4<f32>(positions[index], 0.0, 1.0);
    return output;
}

@fragment
fn fs_blur(input: FullscreenOutput) -> @location(0) vec4<f32> {
    let dimensions = textureDimensions(raw_ssao);
    let size = vec2<i32>(dimensions);
    let pixel = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), size - vec2<i32>(1));

    // The held weapon is rendered with its own overlay depth. It must not enter
    // the world-space AO filter or contribute dark samples around its silhouette.
    if (textureLoad(viewmodel_depth_texture, pixel, 0) < 0.99999) {
        return vec4<f32>(1.0);
    }
    let center_depth = textureLoad(depth_texture, pixel, 0);
    if (center_depth >= 0.99999) {
        return vec4<f32>(1.0);
    }

    let center_normal = normalize(textureLoad(normal_texture, pixel, 0).xyz);
    var ao_sum = 0.0;
    var weight_sum = 0.0;

    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let sample_pixel = clamp(pixel + vec2<i32>(x, y), vec2<i32>(0), size - vec2<i32>(1));
            if (textureLoad(viewmodel_depth_texture, sample_pixel, 0) < 0.99999) {
                continue;
            }
            let sample_depth = textureLoad(depth_texture, sample_pixel, 0);
            if (sample_depth >= 0.99999) {
                continue;
            }

            let sample_normal = normalize(textureLoad(normal_texture, sample_pixel, 0).xyz);
            let offset = vec2<f32>(f32(x), f32(y));
            let spatial_weight = exp(-dot(offset, offset) * 0.5);
            let normal_weight = pow(max(dot(center_normal, sample_normal), 0.0), 16.0);
            let depth_weight = exp(-abs(sample_depth - center_depth) * 500.0);
            let weight = spatial_weight * normal_weight * depth_weight;
            ao_sum += textureLoad(raw_ssao, sample_pixel, 0).r * weight;
            weight_sum += weight;
        }
    }

    var filtered = textureLoad(raw_ssao, pixel, 0).r;
    if (weight_sum > 0.0001) {
        filtered = ao_sum / weight_sum;
    }
    return vec4<f32>(filtered, filtered, filtered, 1.0);
}
