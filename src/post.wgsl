struct PostUniforms {
    bloom: vec4<f32>,
    bloom_extra: vec4<f32>,
    effects: vec4<f32>,
    adaptation: vec4<f32>,
};

@group(0) @binding(0) var<uniform> post: PostUniforms;
@group(0) @binding(1) var hdr_texture: texture_2d<f32>;
@group(0) @binding(2) var bloom_texture: texture_2d<f32>;
@group(0) @binding(3) var post_sampler: sampler;
@group(0) @binding(4) var lens_dirt_texture: texture_2d<f32>;
@group(0) @binding(5) var adaptation_texture: texture_2d<f32>;

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
    // The G-buffer and every effect may render at a lower internal resolution
    // than the swap chain, so sample the HDR buffer with the linear sampler to
    // upscale it into the full-resolution present surface. At scale 1.0 the
    // sample lands on exact texel centres and is identical to a texture load.
    var color = textureSample(hdr_texture, post_sampler, input.uv).rgb;

    let bloom_sample = textureSample(bloom_texture, post_sampler, input.uv).rgb;
    color += bloom_sample * post.bloom.x;
    if (post.bloom_extra.y > 0.0) {
        // Lens dirt is added on top of the bloom instead of masking it, so the
        // dust only brightens its own specks and never carves a dark halo around
        // glowing areas. `bloom_extra.w` normalises the mask to a mean of one.
        let dirt = textureSample(lens_dirt_texture, post_sampler, input.uv).rgb * post.bloom_extra.w;
        color += dirt * bloom_sample * post.bloom_extra.y;
    }

    var exposure = post.effects.y;
    if (post.bloom_extra.z > 0.5) {
        exposure *= textureLoad(adaptation_texture, vec2<i32>(0, 0), 0).r;
    }
    color *= exposure;

    // `adaptation.w` carries height/width so the vignette keeps its shape at any
    // internal render scale.
    let centered = (input.uv - vec2<f32>(0.5)) * vec2<f32>(1.0, post.adaptation.w);
    let vignette = 1.0 - post.effects.x * smoothstep(0.2, 0.72, length(centered));
    color = aces_fitted(color * vignette);
    return vec4<f32>(color, 1.0);
}
