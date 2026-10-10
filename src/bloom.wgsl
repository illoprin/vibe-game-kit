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

const LUMA = vec3<f32>(0.2126, 0.7152, 0.0722);

/// Bright pass with a soft threshold. Below the knee the contribution ramps in
/// smoothly instead of hard-clipping, so bloom fades in on softly lit edges. The
/// surviving colour is then pushed toward higher saturation than the source.
@fragment
fn fs_prefilter(input: FullscreenOutput) -> @location(0) vec4<f32> {
    var color = textureSample(source_texture, post_sampler, input.uv).rgb;
    let threshold = post.bloom.z;
    let knee = max(post.bloom.w, 1e-4);
    let luma = dot(color, LUMA);
    let soft = clamp(luma - threshold + knee, 0.0, 2.0 * knee);
    let contribution = max(luma - threshold, soft * soft / (4.0 * knee));
    color *= contribution / max(luma, 1e-4);

    let saturated_luma = dot(color, LUMA);
    color = mix(vec3<f32>(saturated_luma), color, post.bloom_extra.x);
    return vec4<f32>(color, 1.0);
}

/// Half-resolution box downsample built from five bilinear taps. The radius
/// multiplier widens the footprint, giving a progressively softer blur.
@fragment
fn fs_downsample(input: FullscreenOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source_texture));
    let radius = post.bloom.y * texel;
    var color = textureSample(source_texture, post_sampler, input.uv).rgb * 4.0;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>( radius.x,  radius.y)).rgb;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(-radius.x,  radius.y)).rgb;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>( radius.x, -radius.y)).rgb;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(-radius.x, -radius.y)).rgb;
    return vec4<f32>(color * 0.125, 1.0);
}

/// Nine-tap tent upsample. Runs with additive blending, so the coarser level is
/// added on top of the finer one written by the downsample chain.
@fragment
fn fs_upsample(input: FullscreenOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source_texture));
    let radius = post.bloom.y * texel;
    var color = textureSample(source_texture, post_sampler, input.uv).rgb * 4.0;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(-radius.x, 0.0)).rgb * 2.0;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>( radius.x, 0.0)).rgb * 2.0;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(0.0, -radius.y)).rgb * 2.0;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(0.0,  radius.y)).rgb * 2.0;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(-radius.x, -radius.y)).rgb;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>( radius.x, -radius.y)).rgb;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>(-radius.x,  radius.y)).rgb;
    color += textureSample(source_texture, post_sampler, input.uv + vec2<f32>( radius.x,  radius.y)).rgb;
    return vec4<f32>(color * 0.0625, 1.0);
}
