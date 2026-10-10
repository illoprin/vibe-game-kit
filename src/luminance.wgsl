struct PostUniforms {
    bloom: vec4<f32>,
    bloom_extra: vec4<f32>,
    effects: vec4<f32>,
    adaptation: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: PostUniforms;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var post_sampler: sampler;

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

/// Downsamples the HDR buffer to a small constant size by averaging a 4x4 grid
/// of bilinear taps per destination texel. The result still holds colour; the
/// adaptation pass converts it to a logarithmic average luminance.
@fragment
fn fs_luminance(input: FullscreenOutput) -> @location(0) vec4<f32> {
    var sum = vec3<f32>(0.0);
    for (var y = 0; y < 4; y += 1) {
        for (var x = 0; x < 4; x += 1) {
            let tap = (vec2<f32>(f32(x), f32(y)) + vec2<f32>(0.5)) * 0.25 - vec2<f32>(0.5);
            sum += textureSample(source_texture, post_sampler, input.uv + tap * (1.0 / 64.0)).rgb;
        }
    }
    return vec4<f32>(sum * 0.0625, 1.0);
}
