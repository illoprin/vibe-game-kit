struct PostUniforms {
    bloom: vec4<f32>,
    bloom_extra: vec4<f32>,
    effects: vec4<f32>,
    adaptation: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: PostUniforms;
@group(0) @binding(1) var luminance_texture: texture_2d<f32>;
@group(0) @binding(2) var previous_texture: texture_2d<f32>;

struct FullscreenOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> FullscreenOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    var output: FullscreenOutput;
    let position = positions[index];
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = vec2<f32>(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
    return output;
}

/// Averages the log luminance of the small downsample, derives a target
/// exposure from the configured key, and eases the previous one-pixel exposure
/// toward it. The exponential ease makes dark-to-bright changes faster than
/// bright-to-dark ones, which matches how the eye adapts.
@fragment
fn fs_adapt(input: FullscreenOutput) -> @location(0) vec4<f32> {
    let dimensions = vec2<i32>(textureDimensions(luminance_texture));
    var accumulator = 0.0;
    for (var y = 0; y < dimensions.y; y += 1) {
        for (var x = 0; x < dimensions.x; x += 1) {
            let color = textureLoad(luminance_texture, vec2<i32>(x, y), 0).rgb;
            let luma = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
            accumulator += log2(max(luma, 1e-4));
        }
    }
    let count = f32(dimensions.x * dimensions.y);
    let average = exp2(accumulator / count);

    let exposure_target = clamp(post.effects.z / max(average, 1e-4), post.adaptation.x, post.adaptation.y);
    let previous = textureLoad(previous_texture, vec2<i32>(0, 0), 0).r;
    let rate = 1.0 - exp(-max(post.adaptation.z, 0.0) * max(post.effects.w, 0.0));
    let adapted = mix(previous, exposure_target, rate);
    return vec4<f32>(adapted, adapted, adapted, 1.0);
}
