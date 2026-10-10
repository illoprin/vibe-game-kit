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
@group(0) @binding(1) var albedo_texture: texture_2d<f32>;
@group(0) @binding(2) var normal_texture: texture_2d<f32>;
@group(0) @binding(3) var depth_texture: texture_depth_2d;
@group(0) @binding(4) var shadow_texture: texture_depth_2d;
@group(0) @binding(5) var viewmodel_depth_texture: texture_depth_2d;
@group(0) @binding(6) var shadow_sampler: sampler_comparison;
@group(0) @binding(7) var shadow_moments_texture: texture_2d<f32>;
@group(0) @binding(8) var ssao_texture: texture_2d<f32>;

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

fn bilinear_moments(uv: vec2<f32>) -> vec2<f32> {
    let dimensions = textureDimensions(shadow_moments_texture);
    let texel_position = uv * vec2<f32>(dimensions) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(texel_position));
    let f = fract(texel_position);
    let maximum = vec2<i32>(dimensions) - vec2<i32>(1);
    let p00 = clamp(base, vec2<i32>(0), maximum);
    let p10 = clamp(base + vec2<i32>(1, 0), vec2<i32>(0), maximum);
    let p01 = clamp(base + vec2<i32>(0, 1), vec2<i32>(0), maximum);
    let p11 = clamp(base + vec2<i32>(1, 1), vec2<i32>(0), maximum);
    let a = mix(textureLoad(shadow_moments_texture, p00, 0).rg, textureLoad(shadow_moments_texture, p10, 0).rg, f.x);
    let b = mix(textureLoad(shadow_moments_texture, p01, 0).rg, textureLoad(shadow_moments_texture, p11, 0).rg, f.x);
    return mix(a, b, f.y);
}

fn pcf_visibility(uv: vec2<f32>, receiver_depth: f32, bias: f32) -> f32 {
    let dimensions = vec2<f32>(textureDimensions(shadow_texture));
    let texel = 1.0 / dimensions;
    var visibility = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            visibility += textureSampleCompare(
                shadow_texture,
                shadow_sampler,
                uv + vec2<f32>(f32(x), f32(y)) * texel,
                receiver_depth - bias
            );
        }
    }
    return visibility / 9.0;
}

fn vsm_visibility(uv: vec2<f32>, receiver_depth: f32, bias: f32) -> f32 {
    let moments = bilinear_moments(uv);
    let receiver = receiver_depth - bias;
    if (receiver <= moments.x) {
        return 1.0;
    }
    let variance = max(moments.y - moments.x * moments.x, uniforms.shadow_params.y);
    let difference = receiver - moments.x;
    let probability = variance / (variance + difference * difference);
    let bleed = clamp(uniforms.shadow_params.z, 0.0, 0.99);
    return clamp((probability - bleed) / (1.0 - bleed), 0.0, 1.0);
}

fn shadow_visibility(world_position: vec3<f32>, normal: vec3<f32>) -> f32 {
    let clip = uniforms.light_view_projection * vec4<f32>(world_position, 1.0);
    let ndc = clip.xyz / clip.w;
    if (ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 || ndc.z < 0.0 || ndc.z > 1.0) {
        return 1.0;
    }
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let slope = 1.0 - max(dot(normal, normalize(uniforms.sun_direction.xyz)), 0.0);
    let bias = max(uniforms.shadow_params.w, 0.0001) * (1.0 + slope * 2.0);
    if (uniforms.shadow_params.x < 0.5) {
        return pcf_visibility(uv, ndc.z, bias);
    }
    return vsm_visibility(uv, ndc.z, bias);
}

@fragment
fn fs_lighting(input: FullscreenOutput) -> @location(0) vec4<f32> {
    let size_u = textureDimensions(depth_texture);
    let size = vec2<i32>(size_u);
    let pixel = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), size - vec2<i32>(1));
    let world_depth = textureLoad(depth_texture, pixel, 0);
    let model_depth = textureLoad(viewmodel_depth_texture, pixel, 0);
    let depth = select(world_depth, model_depth, model_depth < 0.99999);
    if (depth >= 0.99999) {
        return vec4<f32>(0.025, 0.04, 0.075, 1.0);
    }
    let albedo = textureLoad(albedo_texture, pixel, 0).rgb;
    let normal = normalize(textureLoad(normal_texture, pixel, 0).xyz);
    let uv = (vec2<f32>(pixel) + vec2<f32>(0.5)) / vec2<f32>(size);
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let homogeneous = uniforms.inverse_view_projection * ndc;
    let world_position = homogeneous.xyz / homogeneous.w;
    let ao = textureLoad(ssao_texture, pixel, 0).r;

    let sun_direction = normalize(uniforms.sun_direction.xyz);
    let sun_diffuse = max(dot(normal, sun_direction), 0.0);
    let shadow = shadow_visibility(world_position, normal);
    var direct_lighting = uniforms.sun_color_intensity.rgb * uniforms.sun_color_intensity.w * sun_diffuse * shadow;
    for (var i = 0; i < 2; i += 1) {
        let to_light = uniforms.point_positions_radius[i].xyz - world_position;
        let distance = length(to_light);
        let radius = max(uniforms.point_positions_radius[i].w, 0.001);
        let attenuation = pow(max(1.0 - distance / radius, 0.0), 2.0) / (1.0 + distance * distance * 0.08);
        let diffuse = max(dot(normal, normalize(to_light)), 0.0);
        direct_lighting += uniforms.point_colors_intensity[i].rgb * uniforms.point_colors_intensity[i].w * attenuation * diffuse;
    }
    // Let SSAO affect a restrained part of direct illumination too, so contact
    // occlusion stays visible even in a brightly lit scene.
    let visibility = mix(1.0, ao, 0.6);
    let lighting = direct_lighting * visibility + vec3<f32>(0.2 * ao);
    return vec4<f32>(albedo * lighting, 1.0);
}
