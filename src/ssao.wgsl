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
@group(0) @binding(1) var normal_texture: texture_2d<f32>;
@group(0) @binding(2) var depth_texture: texture_depth_2d;
@group(0) @binding(3) var viewmodel_depth_texture: texture_depth_2d;

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

fn reconstruct_world(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let world = uniforms.inverse_view_projection * ndc;
    return world.xyz / world.w;
}

@fragment
fn fs_ssao(input: FullscreenOutput) -> @location(0) f32 {
    let dimensions = textureDimensions(depth_texture);
    let size = vec2<i32>(dimensions);
    let pixel = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), size - vec2<i32>(1));
    // Viewmodel depth belongs to a separate camera overlay and is deliberately
    // excluded from SSAO. Treat the weapon pixels as unoccluded instead.
    if (textureLoad(viewmodel_depth_texture, pixel, 0) < 0.99999) {
        return 1.0;
    }
    let center_depth = textureLoad(depth_texture, pixel, 0);
    if (center_depth >= 0.99999) {
        return 1.0;
    }

    let uv = (vec2<f32>(pixel) + vec2<f32>(0.5)) / vec2<f32>(dimensions);
    let origin = reconstruct_world(uv, center_depth);
    let normal = normalize(textureLoad(normal_texture, pixel, 0).xyz);
    let pixel_seed = vec2<f32>(pixel);
    let random_angle = fract(sin(dot(pixel_seed, vec2<f32>(12.9898, 78.233))) * 43758.5453) * 6.2831853;
    let random_vector = vec3<f32>(cos(random_angle), sin(random_angle), 0.0);
    var tangent = random_vector - normal * dot(random_vector, normal);
    if (dot(tangent, tangent) < 0.0001) {
        tangent = cross(normal, vec3<f32>(0.0, 1.0, 0.0));
        if (dot(tangent, tangent) < 0.0001) {
            tangent = cross(normal, vec3<f32>(1.0, 0.0, 0.0));
        }
    }
    tangent = normalize(tangent);
    let bitangent = normalize(cross(normal, tangent));

    let kernel = array<vec3<f32>, 16>(
        vec3<f32>( 0.050,  0.020, 0.998), vec3<f32>(-0.175,  0.102, 0.979),
        vec3<f32>( 0.244, -0.156, 0.957), vec3<f32>(-0.108, -0.321, 0.941),
        vec3<f32>( 0.416,  0.185, 0.890), vec3<f32>(-0.423,  0.247, 0.872),
        vec3<f32>( 0.114,  0.518, 0.848), vec3<f32>(-0.572, -0.154, 0.805),
        vec3<f32>( 0.501, -0.493, 0.711), vec3<f32>( 0.672,  0.307, 0.674),
        vec3<f32>(-0.238,  0.700, 0.674), vec3<f32>(-0.741,  0.380, 0.553),
        vec3<f32>( 0.795, -0.241, 0.556), vec3<f32>( 0.145, -0.820, 0.553),
        vec3<f32>(-0.478, -0.678, 0.558), vec3<f32>( 0.392,  0.772, 0.500)
    );

    let radius = max(uniforms.ssao_params.y, 0.001);
    let depth_bias = max(uniforms.ssao_params.w, 0.001);
    var occlusion = 0.0;
    var valid_samples = 0.0;
    for (var i = 0; i < 16; i += 1) {
        let index = f32(i) / 15.0;
        let sample_scale = mix(0.15, 1.0, index * index);
        let local_direction = kernel[i];
        let sample_direction = normalize(
            tangent * local_direction.x + bitangent * local_direction.y + normal * local_direction.z
        );
        let sample_position = origin + sample_direction * radius * sample_scale;
        let clip = uniforms.view_projection * vec4<f32>(sample_position, 1.0);
        if (clip.w <= 0.0) {
            continue;
        }
        let ndc = clip.xyz / clip.w;
        let sample_uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
        if (ndc.z < 0.0 || ndc.z > 1.0 || any(sample_uv < vec2<f32>(0.0)) || any(sample_uv >= vec2<f32>(1.0))) {
            continue;
        }

        let sample_pixel = clamp(vec2<i32>(sample_uv * vec2<f32>(dimensions)), vec2<i32>(0), size - vec2<i32>(1));
        if (textureLoad(viewmodel_depth_texture, sample_pixel, 0) < 0.99999) {
            continue;
        }
        let observed_depth = textureLoad(depth_texture, sample_pixel, 0);
        if (observed_depth >= 0.99999) {
            continue;
        }
        valid_samples += 1.0;
        let observed_uv = (vec2<f32>(sample_pixel) + vec2<f32>(0.5)) / vec2<f32>(dimensions);
        let observed_position = reconstruct_world(observed_uv, observed_depth);
        let observed_from_origin = observed_position - origin;
        let observed_distance = length(observed_from_origin);
        let sample_distance = length(sample_position - uniforms.camera_position.xyz);
        let observed_camera_distance = length(observed_position - uniforms.camera_position.xyz);
        let is_occluded = observed_distance > depth_bias
            && observed_distance < radius
            && dot(observed_from_origin, sample_position - origin) > 0.0
            && observed_camera_distance < sample_distance - depth_bias;
        if (is_occluded) {
            occlusion += 1.0 - smoothstep(radius * 0.45, radius, observed_distance);
        }
    }

    if (valid_samples < 1.0) {
        return 1.0;
    }
    let raw_visibility = clamp(1.0 - uniforms.ssao_params.x * occlusion / valid_samples, 0.0, 1.0);
    let visibility = pow(raw_visibility, 1.35);
    return visibility;
}
