use egui::{Align2, Context, ViewportId};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use winit::{event::WindowEvent, window::Window};

use crate::renderer::{GraphicsSettings, ShadowFilter};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Pause,
    Graphics,
}

pub struct PauseUi {
    context: Context,
    input: EguiWinitState,
    painter: EguiRenderer,
    tab: Tab,
}

impl PauseUi {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        window: &Window,
    ) -> Self {
        let context = Context::default();
        context.set_visuals(egui::Visuals::dark());
        let input = EguiWinitState::new(
            context.clone(),
            ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let painter = EguiRenderer::new(device, surface_format, None, 1, false);
        Self {
            context,
            input,
            painter,
            tab: Tab::Pause,
        }
    }

    pub fn handle_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.input.on_window_event(window, event).consumed
    }

    pub fn render(
        &mut self,
        window: &Window,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        paused: bool,
        settings: &mut GraphicsSettings,
        map_path: &mut String,
    ) -> (Vec<wgpu::CommandBuffer>, bool, bool) {
        if !paused {
            return (Vec::new(), false, false);
        }
        let previous_tab = self.tab;
        let previous_map_path = map_path.clone();
        let raw_input = self.input.take_egui_input(window);
        let mut resume = false;
        let output = self.context.run(raw_input, |ctx| {
            egui::Window::new("Paused")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .default_width(420.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.tab, Tab::Pause, "Menu");
                        ui.selectable_value(&mut self.tab, Tab::Graphics, "Graphics");
                    });
                    ui.separator();
                    match self.tab {
                        Tab::Pause => {
                            ui.label("Game paused");
                            ui.label("Map to open on next launch");
                            ui.text_edit_singleline(map_path);
                            if ui.button("Resume").clicked() {
                                resume = true;
                            }
                            ui.label("Press Esc to resume or open this menu.");
                        }
                        Tab::Graphics => graphics_panel(ui, settings),
                    }
                });
        });
        self.input
            .handle_platform_output(window, output.platform_output);

        for (id, delta) in &output.textures_delta.set {
            self.painter.update_texture(device, queue, *id, delta);
        }
        let paint_jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let size = window.inner_size();
        let screen = ScreenDescriptor {
            size_in_pixels: [size.width.max(1), size.height.max(1)],
            pixels_per_point: output.pixels_per_point,
        };
        let callback_buffers =
            self.painter
                .update_buffers(device, queue, encoder, &paint_jobs, &screen);
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui pause menu"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.painter
                .render(&mut pass.forget_lifetime(), &paint_jobs, &screen);
        }
        for id in &output.textures_delta.free {
            self.painter.free_texture(id);
        }
        let save_requested = self.tab != previous_tab || *map_path != previous_map_path;
        (callback_buffers, resume, save_requested)
    }
}

fn graphics_panel(ui: &mut egui::Ui, settings: &mut GraphicsSettings) {
    ui.label("Shadow map resolution");
    egui::ComboBox::from_id_salt("shadow_resolution")
        .selected_text(format!(
            "{} × {}",
            settings.shadow_resolution, settings.shadow_resolution
        ))
        .show_ui(ui, |ui| {
            for resolution in [256, 512, 1024, 2048] {
                ui.selectable_value(
                    &mut settings.shadow_resolution,
                    resolution,
                    format!("{resolution} × {resolution}"),
                );
            }
        });
    ui.label("Shadow filtering");
    egui::ComboBox::from_id_salt("shadow_filter")
        .selected_text(match settings.shadow_filter {
            ShadowFilter::Pcf => "PCF (Percentage-Closer Filtering)",
            ShadowFilter::Vsm => "VSM (Variance Shadow Maps)",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut settings.shadow_filter, ShadowFilter::Pcf, "PCF");
            ui.selectable_value(&mut settings.shadow_filter, ShadowFilter::Vsm, "VSM");
        });

    ui.separator();
    ui.checkbox(&mut settings.ssao_enabled, "Enable SSAO");
    if settings.ssao_enabled {
        ui.add(
            egui::Slider::new(&mut settings.ssao_strength, 0.0..=8.0)
                .text("SSAO intensity")
                .step_by(0.1),
        );
        ui.add(
            egui::Slider::new(&mut settings.ssao_radius, 0.1..=5.0)
                .text("SSAO radius")
                .step_by(0.05),
        );
    }
    ui.add(egui::Slider::new(&mut settings.vignette_strength, 0.0..=1.0).text("Vignette"));
    ui.add(egui::Slider::new(&mut settings.exposure, 0.1..=3.0).text("ACES exposure"));
}
