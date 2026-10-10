pub struct TextureTarget {
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// The three essential deferred render targets. Geometry writes unlit HDR
/// colour, world-space normals and scene depth here; every screen-space effect
/// lives in [`PostProcess`] instead so this structure stays minimal.
pub struct GBuffer {
    pub hdr: TextureTarget,
    pub normal: TextureTarget,
    pub depth: TextureTarget,
}

impl GBuffer {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        Self {
            hdr: render_target(
                device,
                "G-buffer HDR colour",
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
            ),
            normal: render_target(
                device,
                "G-buffer world normals",
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
            ),
            depth: depth_target(device, "G-buffer depth", width, height),
        }
    }
}

/// Two interchangeable full-resolution HDR buffers. A screen-space pass writes
/// to one while sampling the other, then [`PingPong::swap`] flips the roles so
/// a whole chain of effects can be chained without ever reading and writing the
/// same texture inside a single pass.
pub struct PingPong {
    front: TextureTarget,
    back: TextureTarget,
}

impl PingPong {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        Self {
            front: hdr_target(device, "post-process ping", width, height),
            back: hdr_target(device, "post-process pong", width, height),
        }
    }

    /// Buffer that the next pass should sample.
    pub fn front(&self) -> &TextureTarget {
        &self.front
    }

    /// Buffer that the next pass should render into.
    ///
    /// Reserved for chaining additional post-processing passes: a pass writes
    /// here, then [`PingPong::swap`] promotes it to the read source.
    #[allow(dead_code)]
    pub fn back(&self) -> &TextureTarget {
        &self.back
    }

    /// Make the most recently written buffer the new read source.
    #[allow(dead_code)]
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.front, &mut self.back);
    }
}

/// Everything extracted from the G-buffer: the HDR ping-pong chain used by
/// deferred lighting and composition, the viewmodel overlay depth, and the
/// single-channel SSAO buffers.
pub struct PostProcess {
    pub ping_pong: PingPong,
    pub viewmodel_depth: TextureTarget,
    pub ssao_raw: TextureTarget,
    pub ssao: TextureTarget,
}

impl PostProcess {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        Self {
            ping_pong: PingPong::new(device, width, height),
            viewmodel_depth: depth_target(device, "viewmodel overlay depth", width, height),
            ssao_raw: ssao_target(device, "raw normal-oriented hemisphere SSAO", width, height),
            ssao: ssao_target(device, "bilaterally blurred SSAO", width, height),
        }
    }
}

pub fn shadow_target(device: &wgpu::Device, resolution: u32) -> TextureTarget {
    depth_target(device, "directional sun shadow map", resolution, resolution)
}

pub fn shadow_moments_target(device: &wgpu::Device, resolution: u32) -> TextureTarget {
    render_target(
        device,
        "variance shadow moments",
        resolution,
        resolution,
        wgpu::TextureFormat::Rgba32Float,
    )
}

fn hdr_target(device: &wgpu::Device, label: &str, width: u32, height: u32) -> TextureTarget {
    render_target(
        device,
        label,
        width,
        height,
        wgpu::TextureFormat::Rgba32Float,
    )
}

/// SSAO only needs one channel of occlusion data, so the raw and blurred
/// buffers use a compact `R8Unorm` target instead of a full HDR one.
fn ssao_target(device: &wgpu::Device, label: &str, width: u32, height: u32) -> TextureTarget {
    render_target(device, label, width, height, wgpu::TextureFormat::R8Unorm)
}

fn render_target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> TextureTarget {
    target(
        device,
        label,
        width,
        height,
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    )
}

fn depth_target(device: &wgpu::Device, label: &str, width: u32, height: u32) -> TextureTarget {
    target(
        device,
        label,
        width,
        height,
        wgpu::TextureFormat::Depth32Float,
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
