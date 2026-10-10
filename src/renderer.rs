use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    path::Path,
    sync::Arc,
};

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, event::WindowEvent, window::Window};

use crate::{
    camera::Camera,
    game::{GameConfig, GameState},
    gpu,
    map::{Map, PointLight, SunLight},
    mesh,
    pipeline::{self, RenderPipelines},
    scene::BuiltScene,
    settings::ControlSettings,
    targets::{
        shadow_moments_target, shadow_target, GBuffer, PostProcess, TextureTarget, MAX_BLOOM_MIPS,
    },
    texture,
    types::{Mesh, PostUniforms, Uniforms},
    ui::{DebugInfo, HudInfo, PauseUi},
};

const VIEWMODEL_FLAG: u32 = 0x8000_0000;
const SHADOW_RESOLUTIONS: [u32; 4] = [256, 512, 1024, 2048];
const LENS_DIRT_PATH: &str = "assets/lens_dirt.png";
/// HDR luminance above which bloom starts, and the width of its soft-threshold
/// knee. These are linear HDR values, so sunlit surfaces (which exceed 1.0)
/// bloom while dimmer highlights fade in gently instead of hard-clipping.
const BLOOM_THRESHOLD: f32 = 1.0;
const BLOOM_KNEE: f32 = 0.5;

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
    pub eye_adaptation_enabled: bool,
    pub eye_adaptation_speed: f32,
    pub eye_adaptation_key: f32,
    pub eye_adaptation_min_exposure: f32,
    pub eye_adaptation_max_exposure: f32,
    pub bloom_enabled: bool,
    pub bloom_intensity: f32,
    pub bloom_passes: u32,
    pub bloom_radius: f32,
    pub bloom_saturation: f32,
    pub lens_dirt_enabled: bool,
    pub lens_dirt_intensity: f32,
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
            eye_adaptation_enabled: false,
            eye_adaptation_speed: 2.0,
            eye_adaptation_key: 0.18,
            eye_adaptation_min_exposure: 0.35,
            eye_adaptation_max_exposure: 3.0,
            bloom_enabled: true,
            bloom_intensity: 0.5,
            bloom_passes: 5,
            bloom_radius: 1.0,
            bloom_saturation: 1.35,
            lens_dirt_enabled: false,
            lens_dirt_intensity: 0.6,
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
    shadow_bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    post_uniform_buffer: wgpu::Buffer,
    shadow_uniform_buffer: wgpu::Buffer,
    post_sampler: wgpu::Sampler,
    /// Kept alive alongside `lens_dirt_view`, which is the only handle used.
    _lens_dirt_texture: wgpu::Texture,
    lens_dirt_view: wgpu::TextureView,
    /// Multiplier that normalises the lens dirt mask to a mean of one.
    lens_dirt_gain: f32,
    world_vertices: wgpu::Buffer,
    world_indices: wgpu::Buffer,
    world_count: u32,
    world_vertex_count: u32,
    weapon_models: HashMap<String, GpuMesh>,
    pickup_models: HashMap<String, Mesh>,
    dynamic_vertices: wgpu::Buffer,
    dynamic_indices: wgpu::Buffer,
    dynamic_vertex_capacity: u64,
    dynamic_index_capacity: u64,
    dynamic_vertex_count: u32,
    dynamic_index_count: u32,
    debug_material: u32,
    gbuffer: GBuffer,
    post_process: PostProcess,
    effects: EffectResources,
    adaptation_front: usize,
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
    pub debug_enabled: bool,
    frame_times: VecDeque<f32>,
    frame_vertex_count: u32,
    frame_draw_calls: u32,
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    vertex_count: u32,
}

/// Bind groups for the screen-space chain. Every entry depends only on the
/// current render targets, so they are rebuilt whenever the targets are
/// recreated (startup and resize) rather than every frame.
struct EffectResources {
    luminance: wgpu::BindGroup,
    adaptation: [wgpu::BindGroup; 2],
    bloom_prefilter: wgpu::BindGroup,
    bloom_downsample: Vec<wgpu::BindGroup>,
    bloom_upsample: Vec<wgpu::BindGroup>,
    post: [wgpu::BindGroup; 2],
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
        let post_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("post process uniforms"),
            contents: bytemuck::bytes_of(&empty_post_uniforms()),
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
        let world_vertex_count = scene.world.vertices.len() as u32;
        let mut weapon_models = HashMap::new();
        for (key, mut model) in scene.weapon_models {
            for vertex in &mut model.vertices {
                vertex.material |= VIEWMODEL_FLAG;
            }
            model.reverse_winding();
            let (vertices, indices, count) =
                gpu::create_mesh_buffers(&device, &format!("held weapon {key}"), &model)?;
            weapon_models.insert(
                key,
                GpuMesh {
                    vertices,
                    indices,
                    count,
                    vertex_count: model.vertices.len() as u32,
                },
            );
        }
        let dynamic_vertices = empty_buffer(
            &device,
            "dynamic world vertices",
            wgpu::BufferUsages::VERTEX,
        );
        let dynamic_indices =
            empty_buffer(&device, "dynamic world indices", wgpu::BufferUsages::INDEX);

        let shadow_resolution = graphics.shadow_resolution;
        let gbuffer = GBuffer::new(&device, config.width, config.height);
        let post_process = PostProcess::new(&device, config.width, config.height);
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
        let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post process linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let (lens_dirt_texture, lens_dirt_view, lens_dirt_gain) =
            create_lens_dirt(&device, &queue, Path::new(LENS_DIRT_PATH));
        let lighting_bind_group = lighting_bind_group(
            &device,
            &pipelines,
            &uniform_buffer,
            &gbuffer,
            &post_process,
            &shadow,
            &shadow_moments,
            &shadow_sampler,
        );
        let ssao_bind_group = ssao_bind_group(
            &device,
            &pipelines,
            &uniform_buffer,
            &gbuffer,
            &post_process,
        );
        let ssao_blur_bind_group = ssao_blur_bind_group(
            &device,
            &pipelines,
            &uniform_buffer,
            &gbuffer,
            &post_process,
        );
        let effects = effect_bind_groups(
            &device,
            &pipelines,
            &post_uniform_buffer,
            &post_process,
            &post_sampler,
            &lens_dirt_view,
        );
        initialize_effect_targets(&device, &queue, &post_process);

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
            shadow_bind_group,
            uniform_buffer,
            post_uniform_buffer,
            shadow_uniform_buffer,
            post_sampler,
            _lens_dirt_texture: lens_dirt_texture,
            lens_dirt_view,
            lens_dirt_gain,
            world_vertices,
            world_indices,
            world_count,
            world_vertex_count,
            weapon_models,
            pickup_models: scene.pickup_models,
            dynamic_vertices,
            dynamic_indices,
            dynamic_vertex_capacity: 4,
            dynamic_index_capacity: 4,
            dynamic_vertex_count: 0,
            dynamic_index_count: 0,
            debug_material: scene.debug_material,
            gbuffer,
            post_process,
            effects,
            adaptation_front: 0,
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
            debug_enabled: false,
            frame_times: VecDeque::with_capacity(600),
            frame_vertex_count: 0,
            frame_draw_calls: 0,
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
        self.recreate_targets();
    }

    fn recreate_targets(&mut self) {
        self.gbuffer = GBuffer::new(&self.device, self.config.width, self.config.height);
        self.post_process = PostProcess::new(&self.device, self.config.width, self.config.height);
        self.ssao_bind_group = ssao_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
            &self.post_process,
        );
        self.ssao_blur_bind_group = ssao_blur_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
            &self.post_process,
        );
        self.lighting_bind_group = lighting_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
            &self.post_process,
            &self.shadow,
            &self.shadow_moments,
            &self.shadow_sampler,
        );
        self.effects = effect_bind_groups(
            &self.device,
            &self.pipelines,
            &self.post_uniform_buffer,
            &self.post_process,
            &self.post_sampler,
            &self.lens_dirt_view,
        );
        self.adaptation_front = 0;
        initialize_effect_targets(&self.device, &self.queue, &self.post_process);
    }

    fn recreate_shadow(&mut self, resolution: u32) {
        self.shadow = shadow_target(&self.device, resolution);
        self.shadow_moments = shadow_moments_target(&self.device, resolution);
        self.lighting_bind_group = lighting_bind_group(
            &self.device,
            &self.pipelines,
            &self.uniform_buffer,
            &self.gbuffer,
            &self.post_process,
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

    fn update_dynamic_mesh(&mut self, game: &GameState) {
        let mut dynamic = Mesh::default();
        for pickup in game.pickups.iter().filter(|pickup| pickup.active) {
            if let Some(template) = self.pickup_models.get(&pickup.kind.model_key()) {
                let mut instance = template.clone();
                instance.transform(GameState::pickup_model_transform(pickup));
                dynamic.append(instance);
            }
        }
        for impact in &game.impacts {
            mesh::add_box(
                &mut dynamic,
                impact.position,
                Vec3::splat(GameState::impact_size()),
                self.debug_material,
            );
        }
        if self.debug_enabled {
            for collider in self.colliders.iter().copied().chain(
                game.pickups
                    .iter()
                    .filter(|pickup| pickup.active)
                    .map(|pickup| pickup.body.collider()),
            ) {
                add_wire_box(&mut dynamic, collider, self.debug_material);
            }
        }

        let vertex_bytes = bytemuck::cast_slice(&dynamic.vertices);
        let index_bytes = bytemuck::cast_slice(&dynamic.indices);
        self.ensure_dynamic_capacity(vertex_bytes.len() as u64, index_bytes.len() as u64);
        if !vertex_bytes.is_empty() {
            self.queue
                .write_buffer(&self.dynamic_vertices, 0, vertex_bytes);
            self.queue
                .write_buffer(&self.dynamic_indices, 0, index_bytes);
        }
        self.dynamic_vertex_count = dynamic.vertices.len() as u32;
        self.dynamic_index_count = dynamic.indices.len() as u32;
    }

    fn ensure_dynamic_capacity(&mut self, vertex_bytes: u64, index_bytes: u64) {
        if vertex_bytes > self.dynamic_vertex_capacity {
            self.dynamic_vertex_capacity = vertex_bytes.next_power_of_two().max(256);
            self.dynamic_vertices = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic world vertices"),
                size: self.dynamic_vertex_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if index_bytes > self.dynamic_index_capacity {
            self.dynamic_index_capacity = index_bytes.next_power_of_two().max(256);
            self.dynamic_indices = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dynamic world indices"),
                size: self.dynamic_index_capacity,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        game_config: &GameConfig,
        game_state: &GameState,
        window: &Window,
        paused: bool,
        dt: f32,
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
        self.graphics.bloom_passes = self.graphics.bloom_passes.clamp(1, MAX_BLOOM_MIPS as u32);
        self.graphics.bloom_intensity = self.graphics.bloom_intensity.clamp(0.0, 8.0);
        self.graphics.bloom_radius = self.graphics.bloom_radius.clamp(0.25, 8.0);
        self.graphics.bloom_saturation = self.graphics.bloom_saturation.clamp(1.0, 3.0);
        self.graphics.lens_dirt_intensity = self.graphics.lens_dirt_intensity.clamp(0.0, 4.0);
        self.graphics.eye_adaptation_speed = self.graphics.eye_adaptation_speed.clamp(0.0, 20.0);
        self.graphics.eye_adaptation_key = self.graphics.eye_adaptation_key.clamp(0.001, 4.0);
        self.graphics.eye_adaptation_min_exposure =
            self.graphics.eye_adaptation_min_exposure.clamp(0.01, 8.0);
        self.graphics.eye_adaptation_max_exposure =
            self.graphics.eye_adaptation_max_exposure.clamp(0.02, 32.0);
        self.update_dynamic_mesh(game_state);
        self.frame_times.push_back(dt.max(0.0));
        let mut history_seconds = self.frame_times.iter().sum::<f32>();
        while history_seconds > 10.0 && self.frame_times.len() > 2 {
            history_seconds -= self.frame_times.pop_front().unwrap_or(0.0);
        }

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
        let weapon = &game_config.weapons[&game_state.selected_weapon];
        let view_model = camera.view_model_with_offset(weapon.fpv_offset, game_state.switch_drop());
        let uniforms = Uniforms {
            view_projection: view_projection.to_cols_array_2d(),
            inverse_view_projection: view_projection.inverse().to_cols_array_2d(),
            light_view_projection: light_view_projection.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            view_model: view_model.to_cols_array_2d(),
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
        let post_uniforms = PostUniforms {
            bloom: [
                if self.graphics.bloom_enabled {
                    self.graphics.bloom_intensity
                } else {
                    0.0
                },
                self.graphics.bloom_radius,
                BLOOM_THRESHOLD,
                BLOOM_KNEE,
            ],
            bloom_extra: [
                self.graphics.bloom_saturation,
                if self.graphics.lens_dirt_enabled {
                    self.graphics.lens_dirt_intensity
                } else {
                    0.0
                },
                if self.graphics.eye_adaptation_enabled {
                    1.0
                } else {
                    0.0
                },
                self.lens_dirt_gain,
            ],
            effects: [
                self.graphics.vignette_strength,
                self.graphics.exposure,
                self.graphics.eye_adaptation_key,
                self.graphics.eye_adaptation_speed,
            ],
            adaptation: [
                self.graphics.eye_adaptation_min_exposure,
                self.graphics.eye_adaptation_max_exposure,
                dt.max(0.0),
                0.0,
            ],
        };
        self.queue.write_buffer(
            &self.post_uniform_buffer,
            0,
            bytemuck::bytes_of(&post_uniforms),
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
        self.render_gbuffer(&mut encoder, &game_state.selected_weapon);
        self.render_ssao(&mut encoder);
        self.render_ssao_blur(&mut encoder);
        self.render_lighting(&mut encoder);
        if self.graphics.eye_adaptation_enabled {
            self.render_luminance(&mut encoder);
            self.render_eye_adaptation(&mut encoder);
            self.adaptation_front = 1 - self.adaptation_front;
        }
        if self.graphics.bloom_enabled {
            self.render_bloom(&mut encoder);
        }
        self.render_post(&mut encoder, &surface_view);
        self.frame_vertex_count = self.world_vertex_count
            + self.dynamic_vertex_count
            + self.weapon_models[&game_state.selected_weapon].vertex_count;
        self.frame_draw_calls = 1 // sun shadow pass
            + 2 // world and viewmodel G-buffer passes
            + if self.dynamic_index_count > 0 { 2 } else { 0 }
            + if self.graphics.ssao_enabled { 2 } else { 0 }
            + if self.graphics.eye_adaptation_enabled { 2 } else { 0 }
            + if self.graphics.bloom_enabled {
                1 + 2 * (self.graphics.bloom_passes - 1)
            } else {
                0
            }
            + 2; // lighting and final composition
        let history = self.frame_times.iter().copied().collect::<Vec<_>>();
        let debug_info = DebugInfo {
            fps: 1.0 / dt.max(0.000_1),
            frametime_ms: dt * 1000.0,
            vertices: self.frame_vertex_count,
            draw_calls: self.frame_draw_calls,
            frametimes_seconds: &history,
        };
        let hud = HudInfo {
            weapon: game_state.selected_weapon_name(game_config),
            health: game_state.health,
            armor: game_state.armor,
            ammo: game_state.selected_ammunition(),
            ammo_max: game_state.selected_weapon_max(game_config),
        };
        let (ui_command_buffers, resume, settings_changed) = self.ui.render(
            window,
            &self.device,
            &self.queue,
            &mut encoder,
            &surface_view,
            paused,
            self.debug_enabled,
            &debug_info,
            &hud,
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
        if self.dynamic_index_count > 0 {
            pass.set_vertex_buffer(0, self.dynamic_vertices.slice(..));
            pass.set_index_buffer(self.dynamic_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.dynamic_index_count, 0, 0..1);
        }
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
            color_attachments: &[Some(color_attachment(
                &self.post_process.ssao_raw.view,
                load,
            ))],
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
                &self.post_process.ssao.view,
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

    fn render_gbuffer(&self, encoder: &mut wgpu::CommandEncoder, selected_weapon: &str) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("G-buffer scene pass"),
                color_attachments: &[
                    Some(color_attachment(
                        &self.gbuffer.hdr.view,
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
            if self.dynamic_index_count > 0 {
                pass.set_vertex_buffer(0, self.dynamic_vertices.slice(..));
                pass.set_index_buffer(self.dynamic_indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.dynamic_index_count, 0, 0..1);
            }
        }
        // Clear depth for the FPS viewmodel, but retain world albedo/normals. It
        // stays visible against walls while remaining part of deferred lighting.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("G-buffer viewmodel pass"),
            color_attachments: &[
                Some(color_attachment(&self.gbuffer.hdr.view, wgpu::LoadOp::Load)),
                Some(color_attachment(
                    &self.gbuffer.normal.view,
                    wgpu::LoadOp::Load,
                )),
            ],
            depth_stencil_attachment: Some(depth_attachment(
                &self.post_process.viewmodel_depth.view,
                wgpu::LoadOp::Clear(1.0),
            )),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.scene);
        pass.set_bind_group(0, &self.scene_bind_group, &[]);
        let weapon = &self.weapon_models[selected_weapon];
        pass.set_vertex_buffer(0, weapon.vertices.slice(..));
        pass.set_index_buffer(weapon.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..weapon.count, 0, 0..1);
    }

    fn render_lighting(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred lighting to HDR buffer"),
            color_attachments: &[Some(color_attachment(
                &self.post_process.ping_pong.front().view,
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

    fn render_luminance(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("average luminance downsample"),
            color_attachments: &[Some(color_attachment(
                &self.post_process.luminance.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.luminance);
        pass.set_bind_group(0, &self.effects.luminance, &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_eye_adaptation(&self, encoder: &mut wgpu::CommandEncoder) {
        let source = self.adaptation_front;
        let destination = 1 - source;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("temporal eye adaptation"),
            color_attachments: &[Some(color_attachment(
                &self.post_process.adaptation[destination].view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.adaptation);
        pass.set_bind_group(0, &self.effects.adaptation[source], &[]);
        pass.draw(0..3, 0..1);
    }

    fn render_bloom(&self, encoder: &mut wgpu::CommandEncoder) {
        let passes = self.graphics.bloom_passes.clamp(1, MAX_BLOOM_MIPS as u32) as usize;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("soft-threshold bloom prefilter"),
                color_attachments: &[Some(color_attachment(
                    &self.post_process.bloom_mips[0].view,
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                ))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipelines.bloom_prefilter);
            pass.set_bind_group(0, &self.effects.bloom_prefilter, &[]);
            pass.draw(0..3, 0..1);
        }
        for level in 1..passes {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom downsample blit"),
                color_attachments: &[Some(color_attachment(
                    &self.post_process.bloom_mips[level].view,
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                ))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipelines.bloom_downsample);
            pass.set_bind_group(0, &self.effects.bloom_downsample[level - 1], &[]);
            pass.draw(0..3, 0..1);
        }
        for level in (1..passes).rev() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bloom additive upsample blit"),
                color_attachments: &[Some(color_attachment(
                    &self.post_process.bloom_mips[level - 1].view,
                    wgpu::LoadOp::Load,
                ))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipelines.bloom_upsample);
            pass.set_bind_group(0, &self.effects.bloom_upsample[level - 1], &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn render_post(&self, encoder: &mut wgpu::CommandEncoder, surface: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bloom composite, eye adaptation, ACES and vignette"),
            color_attachments: &[Some(color_attachment(
                surface,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipelines.post);
        pass.set_bind_group(0, &self.effects.post[self.adaptation_front], &[]);
        pass.draw(0..3, 0..1);
    }
}

fn lighting_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
    post: &PostProcess,
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
            texture_binding(1, &gbuffer.hdr.view),
            texture_binding(2, &gbuffer.normal.view),
            texture_binding(3, &gbuffer.depth.view),
            texture_binding(4, &shadow.view),
            texture_binding(5, &post.viewmodel_depth.view),
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            texture_binding(7, &shadow_moments.view),
            texture_binding(8, &post.ssao.view),
        ],
    })
}

fn ssao_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
    post: &PostProcess,
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
            texture_binding(3, &post.viewmodel_depth.view),
        ],
    })
}

fn ssao_blur_bind_group(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    gbuffer: &GBuffer,
    post: &PostProcess,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bilateral SSAO blur inputs"),
        layout: &pipelines.ssao_blur_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, &post.ssao_raw.view),
            texture_binding(2, &gbuffer.normal.view),
            texture_binding(3, &gbuffer.depth.view),
            texture_binding(4, &post.viewmodel_depth.view),
        ],
    })
}

/// Rebuilds every screen-space bind group. Called at start-up and on resize
/// (the only times the underlying views change), never per frame.
fn effect_bind_groups(
    device: &wgpu::Device,
    pipelines: &RenderPipelines,
    uniforms: &wgpu::Buffer,
    post: &PostProcess,
    sampler: &wgpu::Sampler,
    lens_dirt: &wgpu::TextureView,
) -> EffectResources {
    let hdr = &post.ping_pong.front().view;
    let luminance = sampled_bind_group(
        device,
        &pipelines.bloom_layout,
        "average luminance inputs",
        uniforms,
        hdr,
        sampler,
    );
    let bloom_prefilter = sampled_bind_group(
        device,
        &pipelines.bloom_layout,
        "bloom prefilter inputs",
        uniforms,
        hdr,
        sampler,
    );
    let bloom_downsample = (1..post.bloom_mips.len())
        .map(|level| {
            sampled_bind_group(
                device,
                &pipelines.bloom_layout,
                "bloom downsample inputs",
                uniforms,
                &post.bloom_mips[level - 1].view,
                sampler,
            )
        })
        .collect();
    let bloom_upsample = (0..post.bloom_mips.len() - 1)
        .map(|level| {
            sampled_bind_group(
                device,
                &pipelines.bloom_layout,
                "bloom upsample inputs",
                uniforms,
                &post.bloom_mips[level + 1].view,
                sampler,
            )
        })
        .collect();
    let adaptation = [
        adaptation_bind_group(
            device,
            &pipelines.adaptation_layout,
            uniforms,
            &post.luminance.view,
            &post.adaptation[0].view,
        ),
        adaptation_bind_group(
            device,
            &pipelines.adaptation_layout,
            uniforms,
            &post.luminance.view,
            &post.adaptation[1].view,
        ),
    ];
    let post_groups = [
        post_bind_group(
            device,
            &pipelines.post_layout,
            uniforms,
            hdr,
            &post.bloom_mips[0].view,
            sampler,
            lens_dirt,
            &post.adaptation[0].view,
        ),
        post_bind_group(
            device,
            &pipelines.post_layout,
            uniforms,
            hdr,
            &post.bloom_mips[0].view,
            sampler,
            lens_dirt,
            &post.adaptation[1].view,
        ),
    ];
    EffectResources {
        luminance,
        adaptation,
        bloom_prefilter,
        bloom_downsample,
        bloom_upsample,
        post: post_groups,
    }
}

fn sampled_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    uniforms: &wgpu::Buffer,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, source),
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn adaptation_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    luminance: &wgpu::TextureView,
    previous: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("eye adaptation inputs"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, luminance),
            texture_binding(2, previous),
        ],
    })
}

fn post_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    hdr: &wgpu::TextureView,
    bloom: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    lens_dirt: &wgpu::TextureView,
    adaptation: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bloom composite and tone mapping inputs"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            texture_binding(1, hdr),
            texture_binding(2, bloom),
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            texture_binding(4, lens_dirt),
            texture_binding(5, adaptation),
        ],
    })
}

/// Initialises the screen-space working buffers after creation or resize: the
/// eye-adaptation ping-pong is cleared to a neutral exposure of 1.0 and the
/// bloom chain to black, so the first frame never samples stale or undefined
/// memory (which would otherwise turn the composite into NaNs).
fn initialize_effect_targets(device: &wgpu::Device, queue: &wgpu::Queue, post: &PostProcess) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("initialize screen-space effects"),
    });
    for target in &post.adaptation {
        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear eye adaptation"),
            color_attachments: &[Some(color_attachment(
                &target.view,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    for target in post
        .bloom_mips
        .iter()
        .chain(std::iter::once(&post.luminance))
    {
        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear bloom working target"),
            color_attachments: &[Some(color_attachment(
                &target.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            ))],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    queue.submit(Some(encoder.finish()));
}

/// Loads the lens dirt mask, falling back to a neutral one-pixel white texture
/// so the effect becomes a no-op when the file is absent.
fn create_lens_dirt(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    path: &Path,
) -> (wgpu::Texture, wgpu::TextureView, f32) {
    let loaded = texture::load_lens_dirt(path);
    if loaded.is_none() {
        eprintln!(
            "Lens dirt texture {} could not be loaded; using a neutral mask",
            path.display()
        );
    }
    let (width, height, pixels, gain) =
        loaded.unwrap_or_else(|| (1, 1, vec![255, 255, 255, 255], 1.0));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lens dirt mask"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view, gain)
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

fn empty_post_uniforms() -> PostUniforms {
    PostUniforms {
        bloom: [0.0; 4],
        bloom_extra: [0.0; 4],
        effects: [0.0; 4],
        adaptation: [1.0, 1.0, 0.0, 0.0],
    }
}

fn nearest_shadow_resolution(value: u32) -> u32 {
    SHADOW_RESOLUTIONS
        .iter()
        .copied()
        .min_by_key(|candidate| candidate.abs_diff(value))
        .unwrap_or(1024)
}

fn empty_buffer(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: 4,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn add_wire_box(mesh: &mut Mesh, bounds: crate::types::Aabb, material: u32) {
    let min = bounds.min;
    let max = bounds.max;
    let size = max - min;
    let thickness = 0.012;
    for y in [min.y, max.y] {
        for z in [min.z, max.z] {
            mesh::add_box(
                mesh,
                Vec3::new((min.x + max.x) * 0.5, y, z),
                Vec3::new(size.x + thickness, thickness, thickness),
                material,
            );
        }
    }
    for x in [min.x, max.x] {
        for z in [min.z, max.z] {
            mesh::add_box(
                mesh,
                Vec3::new(x, (min.y + max.y) * 0.5, z),
                Vec3::new(thickness, size.y + thickness, thickness),
                material,
            );
        }
    }
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            mesh::add_box(
                mesh,
                Vec3::new(x, y, (min.z + max.z) * 0.5),
                Vec3::new(thickness, thickness, size.z + thickness),
                material,
            );
        }
    }
}
