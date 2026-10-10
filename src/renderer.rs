use std::{error::Error, sync::Arc};

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, event::WindowEvent, window::Window};

use crate::{
    camera::Camera,
    gpu,
    map::{Map, PointLight, SunLight},
    pipeline::{self, RenderPipelines},
    scene::BuiltScene,
    settings::ControlSettings,
    targets::{shadow_moments_target, shadow_target, GBuffer, TextureTarget},
    texture,
    types::Uniforms,
    ui::PauseUi,
};

const VIEWMODEL_FLAG: u32 = 0x8000_0000;
const SHADOW_RESOLUTIONS: [u32; 4] = [256, 512, 1024, 2048];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShadowFilter {
    Pcf,
    Vsm,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    pub shadow_resolution: u32,
    pub shadow_filter: ShadowFilter,
    pub ssao_enabled: bool,
    pub ssao_strength: f32,
    pub ssao_radius: f32,
    pub vignette_strength: f32,
    pub exposure: f32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            shadow_resolution: 1024,
            shadow_filter: ShadowFilter::Pcf,
            ssao_enabled: true,
            ssao_strength: 2.5,
            ssao_radius: 1.35,
            vignette_strength: 0.28,
            exposure: 1.0,
        }
    }
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipelines: RenderPipelines,
    scene_bind_group: wgpu::BindGroup,
    ssao_bind_group: wgpu::BindGroup,
    ssao_blur_bind_group: wgpu::BindGroup,
    lighting_bind_group: wgpu::BindGroup,
    post_bind_group: wgpu::BindGroup,
    shadow_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    shadow_uniform_buffer: wgpu::Buffer,
    world_vertices: wgpu::Buffer,
    world_indices: wgpu::Buffer,
    world_count: u32,
    view_vertices: wgpu::Buffer,
    view_indices: wgpu::Buffer,
    view_count: u32,
    gbuffer: GBuffer,
    shadow: TextureTarget,
    shadow_moments: TextureTarget,
    shadow_resolution: u32,
    shadow_sampler: wgpu::Sampler,
    sun: SunLight,
    point_lights: Vec<PointLight>,
    ui: PauseUi,
    pub graphics: GraphicsSettings,
    pub colliders: Vec<crate::types::Aabb>,
    pub size: PhysicalSize<u32>,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        map: &Map,
        scene: BuiltScene,
        graphics: GraphicsSettings,
    ) -> Result<Self, Box<dyn Error>> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No compatible WebGPU adapter found")?;
        let hdr_features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba32Float);
        let hdr_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        if !hdr_features.allowed_usages.contains(hdr_usage) {
            return Err("This GPU does not support RGBA32Float HDR render targets".into());
        }
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("OGK deferred renderer device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    ..Default::default()
                },
                None,
            )
            .await?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let pipelines = pipeline::create(&device, format);
        let ui = PauseUi::new(&device, format, &window);
        let material_texture = texture::create_material_array(&device, &queue, &map.materials)?;
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("deferred scene uniforms"),
            contents: bytemuck::bytes_of(&empty_uniforms()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shadow_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sun light view projection"),
            contents: bytemuck::cast_slice(&Mat4::IDENTITY.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let scene_bind_group = gpu::create_bind_group(
            &device,
            &pipelines.scene_layout,
            &material_texture,
            &uniform_buffer,
        );
        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun shadow uniform bind group"),
            layout: &pipelines.shadow_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: shadow_uniform_buffer.as_entire_binding(),
            }],
        });

        let (world_vertices, world_indices, world_count) =
            gpu::create_mesh_buffers(&device, "map geometry", &scene.world)?;
        let mut view_model = scene.viewmodel;
        for vertex in &mut view_model.vertices {
            vertex.material |= VIEWMODEL_FLAG;
        }
        view_model.reverse_winding();
        let (view_vertices, view_indices, view_count) =
            gpu::create_mesh_buffers(&device, "held carbine", &view_model)?;

        let shadow_resolution = graphics.shadow_resolution;
        let gbuffer = GBuffer::new(&device, config.width, config.height);
        let shadow = shadow_target(&device, shadow_resolution);
        let shadow_moments = shadow_moments_target(&device, shadow_resolution);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("PCF shadow comparison sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let lighting_bind_group = lighting_bind_group(
            &device,
            &pipelines,
            &uniform_buffer,
            &gbuffer,
            &shadow,
            &shadow_moments,
            &shadow_sampler,
        );
        let ssao_bind_group = ssao_bind_group(&device, &pipelines, &uniform_buffer, &gbuffer);
        let ssao_blur_bind_group =
            ssao_blur_bind_group(&device, &pipelines, &uniform_buffer, &gbuffer);
        let post_bind_group = post_bind_group(&device, &pipelines, &uniform_buffer, &gbuffer);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipelines,
            scene_bind_group,
            ssao_bind_group,
            ssao_blur_bind_group,
            lighting_bind_group,
            post_bind_group,
            shadow_bind_group,
            uniform_buffer,
            shadow_uniform_buffer,
            world_vertices,
            world_indices,
            world_count,
            view_vertices,
            view_indices,
            view_count,
            gbuffer,
            shadow,
            shadow_moments,
            shadow_resolution,
            shadow_sampler,
            sun: map.sun,
            point_lights: map.point_lights.clone(),
            ui,
            graphics,
            colliders: scene.colliders,
            size,
        })
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.size = size;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.recreate_gbuffer();
    }

    fn recreate_gbuffer(&mut self) {
        self.gbuffer = GBuffer::new(&self.device, self.config.width, self.config.height);
        self.ssao_bind_group = ssao_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
        );
        self.ssao_blur_bind_group = ssao_blur_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
        );
        self.lighting_bind_group = lighting_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
            &self.shadow,
            &self.shadow_moments,
            &self.shadow_sampler,
        );
        self.post_bind_group = post_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
        );
    }

    fn recreate_shadow(&mut self, resolution: u32) {
        self.shadow = shadow_target(&self.device, resolution);
        self.shadow_moments = shadow_moments_target(&self.device, resolution);
        self.lighting_bind_group = lighting_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
            &self.shadow,
            &self.shadow_moments,
            &self.shadow_sampler,
        );
        self.graphics.shadow_resolution = resolution;
        self.shadow_resolution = resolution;
    }

    pub fn handle_ui_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.ui.handle_event(window, event)
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        window: &Window,
        paused: bool,
        controls: &mut ControlSettings,
        map_path: &mut String,
    ) -> Result<(bool, bool), wgpu::SurfaceError> {
        self.graphics.shadow_resolution =
            nearest_shadow_resolution(self.graphics.shadow_resolution);
        if self.shadow_resolution != self.graphics.shadow_resolution {
            self.recreate_shadow(self.graphics.shadow_resolution);
        }
        self.graphics.ssao_strength = self.graphics.ssao_strength.clamp(0.0, 8.0);
        self.graphics.ssao_radius = self.graphics.ssao_radius.clamp(0.1, 5.0);
        self.graphics.vignette_strength = self.graphics.vignette_strength.clamp(0.0, 1.0);
        self.graphics.exposure = self.graphics.exposure.clamp(0.1, 3.0);

        let aspect = self.config.width as f32 / self.config.height as f32;
        let view_projection = camera.view_projection(aspect);
        let light_view_projection = sunlight_view_projection(camera.position, self.sun.direction);
        let mut point_positions_radius = [[0.0; 4]; 2];
        let mut point_colors_intensity = [[0.0; 4]; 2];
        for (index, light) in self.point_lights.iter().take(2).enumerate() {
            point_positions_radius[index] = [
                light.position[0],
                light.position[1],
                light.position[2],
                light.radius,
            ];
            point_colors_intensity[index] = [
                light.color[0],
                light.color[1],
                light.color[2],
                light.intensity,
            ];
        }
        let uniforms = Uniforms {
            view_projection: view_projection.to_cols_array_2d(),
            inverse_view_projection: view_projection.inverse().to_cols_array_2d(),
            light_view_projection: light_view_projection.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            view_model: camera.view_model().to_cols_array_2d(),
            camera_position: (camera.render_position() + Vec3::Y * 1.62)
                .extend(1.0)
                .to_array(),
            sun_direction: [
                self.sun.direction[0],
                self.sun.direction[1],
                self.sun.direction[2],
                0.0,
            ],
            sun_color_intensity: [
                self.sun.color[0],
                self.sun.color[1],
                self.sun.color[2],
                self.sun.intensity,
            ],
            point_positions_radius,
            point_colors_intensity,
            ssao_params: [
                self.graphics.ssao_strength,
                self.graphics.ssao_radius,
                if self.graphics.ssao_enabled { 1.0 } else { 0.0 },
                0.025,
            ],
            shadow_params: [
                if self.graphics.shadow_filter == ShadowFilter::Vsm {
                    1.0
                } else {
                    0.0
                },
                0.00002,
                0.2,
                0.0008,
            ],
            effects: [
                self.graphics.vignette_strength,
                self.graphics.exposure,
                0.0,
                0.0,
            ],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.queue.write_buffer(
            &self.shadow_uniform_buffer,
            0,
            bytemuck::cast_slice(&light_view_projection.to_cols_array()),
        );

        let frame = self.surface.get_current_texture()?;
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("deferred frame encoder"),
            });

        self.render_shadows(&mut encoder);
        self.render_gbuffer(&mut encoder);
        self.render_ssao(&mut encoder);
        self.render_ssao_blur(&mut encoder);
        self.render_lighting(&mut encoder);
        self.render_post(&mut encoder, &surface_view);
        let (ui_command_buffers, resume, settings_changed) = self.ui.render(
            window,
            &self.device,
            &self.queue,
            &mut encoder,
            &surface_view,
            paused,
            &mut self.graphics,
            controls,
            map_path,
        );

        self.queue
            .submit(ui_command_buffers.into_iter().chain([encoder.finish()]));
        frame.present();
        Ok((resume, settings_changed))
    }

    fn render_shadows(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sun shadow depth and VSM moments pass"),
            color_attachments: &[Some(color_attachment(
                &self.shadow_moments.view,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            ))],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.shadow);
        pass.set_bind_group(0, &self.shadow_bind_group, &[]);
        pass.set_vertex_buffer(0, self.world_vertices.slice(..));
        pass.set_index_buffer(self.world_indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.world_count, 0, 0..1);
    }

    fn render_ssao(&self, encoder: &mut wgpu::CommandEncoder) {
        let load = if self.graphics.ssao_enabled {
            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
        } else {
            wgpu::LoadOp::Clear(wgpu::Color::WHITE)
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(if self.graphics.ssao_enabled {
                "normal-oriented hemisphere SSAO pass"
            } else {
                "clear disabled SSAO to fully visible"
            }),
            color_attachments: &[Some(color_attachment(&self.gbuffer.ssao_raw.view, load))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if self.graphics.ssao_enabled {
            pass.set_pipeline(&self.pipelines.ssao);
            pass.set_bind_group(0, &self.ssao_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn render_ssao_blur(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("edge-aware bilateral SSAO blur pass"),
            color_attachments: &[Some(color_attachment(
                &self.gbuffer.ssao.view,
                if self.graphics.ssao_enabled {
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                } else {
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                },
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if self.graphics.ssao_enabled {
            pass.set_pipeline(&self.pipelines.ssao_blur);
            pass.set_bind_group(0, &self.ssao_blur_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn render_gbuffer(&self, encoder: &mut wgpu::CommandEncoder) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("G-buffer scene pass"),
                color_attachments: &[
                    Some(color_attachment(
                        &self.gbuffer.albedo.view,
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    )),
                    Some(color_attachment(
                        &self.gbuffer.normal.view,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    )),
                ],
                depth_stencil_attachment: Some(depth_attachment(
                    &self.gbuffer.depth.view,
                    wgpu::LoadOp::Clear(1.0),
                )),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipelines.scene);
            pass.set_bind_group(0, &self.scene_bind_group, &[]);
            pass.set_vertex_buffer(0, self.world_vertices.slice(..));
            pass.set_index_buffer(self.world_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.world_count, 0, 0..1);
        }
        // Clear depth for the FPS viewmodel, but retain world albedo/normals. It
        // stays visible against walls while remaining part of deferred lighting.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("G-buffer viewmodel pass"),
            color_attachments: &[
                Some(color_attachment(
                    &self.gbuffer.albedo.view,
                    wgpu::LoadOp::Load,
                )),
                Some(color_attachment(
                    &self.gbuffer.normal.view,
                    wgpu::LoadOp::Load,
                )),
            ],
            depth_stencil_attachment: Some(depth_attachment(
                &self.gbuffer.viewmodel_depth.view,
                wgpu::LoadOp::Clear(1.0),
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.scene);
        pass.set_bind_group(0, &self.scene_bind_group, &[]);
        pass.set_vertex_buffer(0, self.view_vertices.slice(..));
        pass.set_index_buffer(self.view_indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.view_count, 0, 0..1);
    }

    fn render_lighting(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred lighting to HDR buffer"),
            color_attachments: &[Some(color_attachment(
                &self.gbuffer.hdr.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.lighting);
        pass.set_bind_group(0, &self.lighting_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_post(&self, encoder: &mut wgpu::CommandEncoder, surface: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ACES tone mapping and vignette"),
            color_attachments: &[Some(color_attachment(
                surface,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.post);
        pass.set_bind_group(0, &self.post_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn lighting_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
    shadow: &TextureTarget,
    shadow_moments: &TextureTarget,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("G-buffer and shadow lighting inputs"),
        layout: &pipelines.lighting_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, &gbuffer.albedo.view),
            texture_binding(2, &gbuffer.normal.view),
            texture_binding(3, &gbuffer.depth.view),
            texture_binding(4, &shadow.view),
            texture_binding(5, &gbuffer.viewmodel_depth.view),
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            texture_binding(7, &shadow_moments.view),
            texture_binding(8, &gbuffer.ssao.view),
        ],
    })
}

fn ssao_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hemisphere SSAO G-buffer inputs"),
        layout: &pipelines.ssao_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, &gbuffer.normal.view),
            texture_binding(2, &gbuffer.depth.view),
            texture_binding(3, &gbuffer.viewmodel_depth.view),
        ],
    })
}

fn ssao_blur_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bilateral SSAO blur inputs"),
        layout: &pipelines.ssao_blur_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, &gbuffer.ssao_raw.view),
            texture_binding(2, &gbuffer.normal.view),
            texture_binding(3, &gbuffer.depth.view),
            texture_binding(4, &gbuffer.viewmodel_depth.view),
        ],
    })
}

fn post_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("HDR post processing input"),
        layout: &pipelines.post_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, &gbuffer.hdr.view),
        ],
    })
}

fn texture_binding(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

fn color_attachment<'a>(
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'a> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}

fn depth_attachment<'a>(
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<f32>,
) -> wgpu::RenderPassDepthStencilAttachment<'a> {
    wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    }
}

fn sunlight_view_projection(camera: Vec3, direction: [f32; 3]) -> Mat4 {
    let light_direction = Vec3::from_array(direction).normalize_or_zero();
    let center = camera + Vec3::Y * 1.0;
    let eye = center + light_direction * 45.0;
    let up = if light_direction.dot(Vec3::Y).abs() > 0.96 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Mat4::orthographic_rh(-24.0, 24.0, -24.0, 24.0, 0.1, 100.0) * Mat4::look_at_rh(eye, center, up)
}

fn empty_uniforms() -> Uniforms {
    Uniforms {
        view_projection: Mat4::IDENTITY.to_cols_array_2d(),
        inverse_view_projection: Mat4::IDENTITY.to_cols_array_2d(),
        light_view_projection: Mat4::IDENTITY.to_cols_array_2d(),
        model: Mat4::IDENTITY.to_cols_array_2d(),
        view_model: Mat4::IDENTITY.to_cols_array_2d(),
        camera_position: [0.0; 4],
        sun_direction: [0.0; 4],
        sun_color_intensity: [0.0; 4],
        point_positions_radius: [[0.0; 4]; 2],
        point_colors_intensity: [[0.0; 4]; 2],
        ssao_params: [0.0; 4],
        shadow_params: [0.0; 4],
        effects: [0.0; 4],
    }
}

fn nearest_shadow_resolution(value: u32) -> u32 {
    SHADOW_RESOLUTIONS
        .iter()
        .copied()
        .min_by_key(|candidate| candidate.abs_diff(value))
        .unwrap_or(1024)
}
