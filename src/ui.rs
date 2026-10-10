use egui::{Align2, Context, ViewportId};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use winit::{event::WindowEvent, window::Window};

use crate::{
    renderer::{GraphicsSettings, ShadowFilter},
    settings::ControlSettings,
};

pub struct DebugInfo<'a> {
    pub fps: f32,
    pub frametime_ms: f32,
    pub vertices: u32,
    pub draw_calls: u32,
    pub frametimes_seconds: &'a [f32],
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Pause,
    Controls,
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
        context.set_zoom_factor(1.5);
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
        debug_enabled: bool,
        debug: &DebugInfo<'_>,
        hud: &str,
        settings: &mut GraphicsSettings,
        controls: &mut ControlSettings,
        map_path: &mut String,
    ) -> (Vec<wgpu::CommandBuffer>, bool, bool) {
        if !paused && !debug_enabled && hud.is_empty() {
            return (Vec::new(), false, false);
        }
        let previous_tab = self.tab;
        let previous_map_path = map_path.clone();
        let previous_sensitivity = controls.mouse_sensitivity;
        let raw_input = self.input.take_egui_input(window);
        let mut resume = false;
        let output = self.context.run(raw_input, |ctx| {
            if paused {
                egui::Window::new("Paused")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .default_width(420.0)
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.tab, Tab::Pause, "Menu");
                            ui.selectable_value(&mut self.tab, Tab::Controls, "Controls");
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
                            Tab::Controls => controls_panel(ui, controls),
                            Tab::Graphics => graphics_panel(ui, settings),
                        }
                    });
            }
            if debug_enabled {
                debug_panel(ctx, debug);
            }
            if !hud.is_empty() && !paused {
                egui::Area::new("game_hud".into())
                    .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -14.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::default()
                            .fill(egui::Color32::from_black_alpha(170))
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .corner_radius(5.0)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(hud).color(egui::Color32::WHITE));
                            });
                    });
            }
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
        let save_requested = self.tab != previous_tab
            || *map_path != previous_map_path
            || controls.mouse_sensitivity != previous_sensitivity;
        (callback_buffers, resume, save_requested)
    }
}

fn debug_panel(ctx: &Context, debug: &DebugInfo<'_>) {
    egui::Area::new("debug_panel".into())
        .fixed_pos(egui::pos2(10.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_black_alpha(205))
                .inner_margin(egui::Margin::same(10))
                .corner_radius(5.0)
                .show(ui, |ui| {
                    ui.set_min_width(300.0);
                    ui.label(format!(
                        "FPS: {:.0}  |  Frametime: {:.2} ms",
                        debug.fps, debug.frametime_ms
                    ));
                    ui.label(format!(
                        "Vertices: {}  |  Draw Calls: {}",
                        debug.vertices, debug.draw_calls
                    ));
                    ui.add_space(4.0);
                    frametime_chart(ui, debug.frametimes_seconds);
                });
        });
}

fn frametime_chart(ui: &mut egui::Ui, frametimes: &[f32]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 78.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(28));
    painter.text(
        rect.left_top() + egui::vec2(5.0, 3.0),
        egui::Align2::LEFT_TOP,
        "Frametime • 10 sec",
        egui::FontId::proportional(10.0),
        egui::Color32::LIGHT_GRAY,
    );
    if frametimes.len() < 2 {
        return;
    }
    let max_ms = frametimes
        .iter()
        .map(|time| time * 1000.0)
        .fold(16.67f32, f32::max)
        .min(100.0);
    let graph = rect.shrink2(egui::vec2(4.0, 5.0));
    let points = frametimes.len() - 1;
    for index in 0..points {
        let x0 = graph.left() + graph.width() * index as f32 / points as f32;
        let x1 = graph.left() + graph.width() * (index + 1) as f32 / points as f32;
        let y0 =
            graph.bottom() - (frametimes[index] * 1000.0 / max_ms).clamp(0.0, 1.0) * graph.height();
        let y1 = graph.bottom()
            - (frametimes[index + 1] * 1000.0 / max_ms).clamp(0.0, 1.0) * graph.height();
        painter.line_segment(
            [egui::pos2(x0, y0), egui::pos2(x1, y1)],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(88, 210, 150)),
        );
    }
}

fn controls_panel(ui: &mut egui::Ui, controls: &mut ControlSettings) {
    ui.label("Mouse");
    ui.add(
        egui::Slider::new(&mut controls.mouse_sensitivity, 0.1..=3.0)
            .text("Sensitivity")
            .step_by(0.05),
    );
    ui.label("Hold Space to auto-jump when landing.");
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
