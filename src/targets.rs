pub struct TextureTarget {
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

pub struct GBuffer {
    pub albedo: TextureTarget,
    pub normal: TextureTarget,
    pub depth: TextureTarget,
    pub viewmodel_depth: TextureTarget,
    pub ssao_raw: TextureTarget,
    pub ssao: TextureTarget,
    pub hdr: TextureTarget,
}

impl GBuffer {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        Self {
            albedo: target(
                device,
                "G-buffer albedo",
                width,
                height,
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            normal: target(
                device,
                "G-buffer world normals",
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            depth: target(
                device,
                "G-buffer depth",
                width,
                height,
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            viewmodel_depth: target(
                device,
                "viewmodel overlay depth",
                width,
                height,
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            ssao_raw: target(
                device,
                "raw normal-oriented hemisphere SSAO",
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            ssao: target(
                device,
                "bilaterally blurred SSAO",
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            hdr: target(
                device,
                "HDR lighting (RGBA32Float)",
                width,
                height,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        }
    }
}

pub fn shadow_target(device: &wgpu::Device, resolution: u32) -> TextureTarget {
    target(
        device,
        "directional sun shadow map",
        resolution,
        resolution,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    )
}

pub fn shadow_moments_target(device: &wgpu::Device, resolution: u32) -> TextureTarget {
    target(
        device,
        "variance shadow moments",
        resolution,
        resolution,
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    )
}

fn target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> TextureTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    TextureTarget {
        _texture: texture,
        view,
    }
}
