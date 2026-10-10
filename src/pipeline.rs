use crate::types::Vertex;

pub struct RenderPipelines {
    pub scene_layout: wgpu::BindGroupLayout,
    pub scene: wgpu::RenderPipeline,
    pub shadow_layout: wgpu::BindGroupLayout,
    pub shadow: wgpu::RenderPipeline,
    pub ssao_layout: wgpu::BindGroupLayout,
    pub ssao: wgpu::RenderPipeline,
    pub ssao_blur_layout: wgpu::BindGroupLayout,
    pub ssao_blur: wgpu::RenderPipeline,
    pub lighting_layout: wgpu::BindGroupLayout,
    pub lighting: wgpu::RenderPipeline,
    pub post_layout: wgpu::BindGroupLayout,
    pub post: wgpu::RenderPipeline,
}

pub fn create(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> RenderPipelines {
    let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene G-buffer bindings"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
            texture_entry(1, wgpu::TextureViewDimension::D2Array, true),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let scene = vertex_pipeline(
        device,
        "G-buffer geometry",
        include_str!("shader.wgsl"),
        "vs_main",
        Some("fs_main"),
        &scene_layout,
        &[
            Some(color_target(wgpu::TextureFormat::Rgba16Float)),
            Some(color_target(wgpu::TextureFormat::Rgba16Float)),
        ],
        Some(wgpu::TextureFormat::Depth32Float),
        true,
    );

    let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sun shadow bindings"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
    });
    let shadow = vertex_pipeline(
        device,
        "sun shadow depth",
        include_str!("shadow.wgsl"),
        "vs_shadow",
        Some("fs_shadow"),
        &shadow_layout,
        &[Some(color_target(wgpu::TextureFormat::Rgba32Float))],
        Some(wgpu::TextureFormat::Depth32Float),
        true,
    );

    let ssao_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("normal-oriented hemisphere SSAO inputs"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, wgpu::TextureViewDimension::D2, false),
            depth_entry(2),
            depth_entry(3),
        ],
    });
    let ssao = fullscreen_pipeline(
        device,
        "normal-oriented hemisphere SSAO pass",
        include_str!("ssao.wgsl"),
        "fs_ssao",
        &ssao_layout,
        wgpu::TextureFormat::R8Unorm,
    );

    let ssao_blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bilateral SSAO blur inputs"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, wgpu::TextureViewDimension::D2, false),
            texture_entry(2, wgpu::TextureViewDimension::D2, false),
            depth_entry(3),
            depth_entry(4),
        ],
    });
    let ssao_blur = fullscreen_pipeline(
        device,
        "edge-aware SSAO blur pass",
        include_str!("ssao_blur.wgsl"),
        "fs_blur",
        &ssao_blur_layout,
        wgpu::TextureFormat::R8Unorm,
    );

    let lighting_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("deferred lighting bindings"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, wgpu::TextureViewDimension::D2, false),
            texture_entry(2, wgpu::TextureViewDimension::D2, false),
            depth_entry(3),
            depth_entry(4),
            depth_entry(5),
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            texture_entry(7, wgpu::TextureViewDimension::D2, false),
            texture_entry(8, wgpu::TextureViewDimension::D2, false),
        ],
    });
    let lighting = fullscreen_pipeline(
        device,
        "deferred HDR lighting",
        include_str!("lighting.wgsl"),
        "fs_lighting",
        &lighting_layout,
        wgpu::TextureFormat::Rgba32Float,
    );

    let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("post processing bindings"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
            texture_entry(1, wgpu::TextureViewDimension::D2, false),
        ],
    });
    let post = fullscreen_pipeline(
        device,
        "ACES and vignette post processing",
        include_str!("post.wgsl"),
        "fs_post",
        &post_layout,
        surface_format,
    );

    RenderPipelines {
        scene_layout,
        scene,
        shadow_layout,
        shadow,
        ssao_layout,
        ssao,
        ssao_blur_layout,
        ssao_blur,
        lighting_layout,
        lighting,
        post_layout,
        post,
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(
    binding: u32,
    dimension: wgpu::TextureViewDimension,
    filterable: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn depth_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn color_target(format: wgpu::TextureFormat) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    }
}

fn vertex_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    vertex_entry: &str,
    fragment_entry: Option<&str>,
    bind_layout: &wgpu::BindGroupLayout,
    targets: &[Option<wgpu::ColorTargetState>],
    depth_format: Option<wgpu::TextureFormat>,
    cull_back: bool,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[bind_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(vertex_entry),
            compilation_options: Default::default(),
            buffers: &[Vertex::layout()],
        },
        fragment: fragment_entry.map(|entry_point| wgpu::FragmentState {
            module: &shader,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            targets,
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: cull_back.then_some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: Default::default(),
            bias: if label.contains("shadow") {
                wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                }
            } else {
                Default::default()
            },
        }),
        multisample: Default::default(),
        multiview: None,
        cache: None,
    })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    fragment_entry: &str,
    bind_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[bind_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(color_target(format))],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn render_shaders_are_valid_wgsl() {
        for source in [
            include_str!("shader.wgsl"),
            include_str!("shadow.wgsl"),
            include_str!("ssao.wgsl"),
            include_str!("ssao_blur.wgsl"),
            include_str!("lighting.wgsl"),
            include_str!("post.wgsl"),
        ] {
            let module = naga::front::wgsl::parse_str(source).expect("WGSL should parse");
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .expect("WGSL should validate");
        }
    }
}
