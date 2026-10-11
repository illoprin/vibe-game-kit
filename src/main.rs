mod camera;
mod collision;
mod game;
mod gpu;
mod map;
mod mesh;
mod model;
mod obj;
mod pipeline;
mod renderer;
mod rigidbody;
mod scene;
mod settings;
mod targets;
mod texture;
mod types;
mod ui;

use std::{collections::HashSet, error::Error, path::PathBuf, sync::Arc, time::Instant};

use camera::Camera;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut app_settings = settings::AppSettings::load(settings::SETTINGS_FILE);
    let mut game_file = PathBuf::from(game::DEFAULT_GAME_FILE);
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--game" | "-g" => {
                game_file = arguments
                    .next()
                    .ok_or("--game requires a game.toml path")?
                    .into();
            }
            "--map" | "-m" => {
                app_settings.map_path = arguments.next().ok_or("--map requires a map path")?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: vibe-game-kit [game.toml] [--game <game.toml>] [--map <map file>]"
                );
                return Ok(());
            }
            positional if !positional.starts_with('-') => {
                if PathBuf::from(positional)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("map"))
                {
                    app_settings.map_path = positional.to_string();
                } else {
                    game_file = positional.into();
                }
            }
            other => return Err(format!("unknown command-line argument: {other}").into()),
        }
    }
    if app_settings.map_path.trim().is_empty() {
        app_settings.map_path = settings::AppSettings::default().map_path;
    }
    let game_config = game::GameConfig::load(&game_file)?;
    let map_file = app_settings.map_path.clone();
    let mut map = map::Map::load(&map_file, &game_config.name)?;
    for object in &map.objects {
        if let map::MapObject::Model { path, .. } = object {
            model::register_adjacent_textures(path, &mut map.materials);
        }
    }
    model::register_adjacent_textures(&map.viewmodel, &mut map.materials);
    for path in game_config.model_paths() {
        model::register_adjacent_textures(&path, &mut map.materials);
    }
    if !map
        .materials
        .iter()
        .any(|material| material.name == "debug_red")
    {
        map.materials.push(map::MaterialDef {
            name: "debug_red".into(),
            texture: PathBuf::from("assets/debug_red.ppm"),
        });
    }
    let built_scene = scene::build(&mut map, &game_config)?;
    let game_state = game::GameState::new(&game_config, &map)?;
    let mut app = GameApp {
        app_settings,
        game_config,
        map: Some(map),
        built_scene: Some(built_scene),
        game_state,
        window: None,
        renderer: None,
        camera: Camera::new(),
        pressed: HashSet::new(),
        captured: false,
        firing: false,
        paused: false,
        previous_frame: Instant::now(),
    };
    EventLoop::new()?.run_app(&mut app)?;
    Ok(())
}

struct GameApp {
    app_settings: settings::AppSettings,
    game_config: game::GameConfig,
    map: Option<map::Map>,
    built_scene: Option<scene::BuiltScene>,
    game_state: game::GameState,
    window: Option<Arc<Window>>,
    renderer: Option<renderer::Renderer>,
    camera: Camera,
    pressed: HashSet<KeyCode>,
    captured: bool,
    firing: bool,
    paused: bool,
    previous_frame: Instant,
}

impl GameApp {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(format!("{} • OGK", self.game_config.name))
                    .with_inner_size(PhysicalSize::new(1280, 800)),
            )?,
        );
        let map = self.map.as_ref().ok_or("map was already consumed")?;
        let built_scene = self
            .built_scene
            .take()
            .ok_or("scene was already consumed")?;
        let renderer = pollster::block_on(renderer::Renderer::new(
            window.clone(),
            map,
            built_scene,
            self.app_settings.graphics.clone(),
        ))?;
        persist_settings(&mut self.app_settings, &renderer);
        self.window = Some(window);
        self.renderer = Some(renderer);
        Ok(())
    }
}

impl ApplicationHandler for GameApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Poll);
        if self.window.is_none() {
            if let Err(error) = self.initialize(event_loop) {
                eprintln!("Could not initialize the game: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) else {
            return;
        };
        if window_id != window.id() {
            return;
        }
        let _ = renderer.handle_ui_event(window, &event);
        match event {
            WindowEvent::CloseRequested => {
                persist_settings(&mut self.app_settings, renderer);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => renderer.resize(size),
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::Escape
                        && event.state == ElementState::Pressed
                        && !event.repeat
                    {
                        self.paused = !self.paused;
                        self.pressed.clear();
                        self.firing = false;
                        if self.paused {
                            let _ = window.set_cursor_grab(CursorGrabMode::None);
                            window.set_cursor_visible(true);
                            self.captured = false;
                        } else {
                            self.captured = window.set_cursor_grab(CursorGrabMode::Locked).is_ok()
                                || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
                            if self.captured {
                                window.set_cursor_visible(false);
                            }
                        }
                        persist_settings(&mut self.app_settings, renderer);
                    } else if event.state == ElementState::Pressed {
                        if code == KeyCode::F2 && !event.repeat {
                            renderer.request_screenshot();
                        }
                        if !self.paused && !event.repeat {
                            if code == KeyCode::F1 {
                                renderer.debug_enabled = !renderer.debug_enabled;
                            }
                            if self.captured {
                                if let Some(slot) = weapon_slot(code) {
                                    self.game_state.select_slot(slot);
                                }
                            }
                        }
                        if !self.paused {
                            self.pressed.insert(code);
                        }
                    } else {
                        self.pressed.remove(&code);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !self.captured && !self.paused => {
                self.captured = window.set_cursor_grab(CursorGrabMode::Locked).is_ok()
                    || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
                if self.captured {
                    window.set_cursor_visible(false);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if self.captured && !self.paused => self.firing = true,
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.firing = false,
            WindowEvent::MouseWheel { delta, .. } if self.captured && !self.paused => {
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32,
                };
                if amount.abs() > f32::EPSILON {
                    self.game_state
                        .cycle_weapon(if amount > 0.0 { 1 } else { -1 });
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let frame_dt = now.duration_since(self.previous_frame).as_secs_f32();
                let dt = frame_dt.min(0.05);
                self.previous_frame = now;
                if !self.paused {
                    collision::update_player(
                        &mut self.camera,
                        &self.pressed,
                        &renderer.colliders,
                        dt,
                    );
                    self.game_state.update(
                        dt,
                        &mut self.camera,
                        &self.game_config,
                        self.firing,
                        &renderer.colliders,
                    );
                }
                match renderer.render(
                    &self.camera,
                    &self.game_config,
                    &self.game_state,
                    window,
                    self.paused,
                    frame_dt,
                    &mut self.app_settings.controls,
                    &mut self.app_settings.map_path,
                ) {
                    Ok((resume_requested, settings_changed)) => {
                        if settings_changed || resume_requested {
                            persist_settings(&mut self.app_settings, renderer);
                        }
                        if resume_requested {
                            self.paused = false;
                            self.captured = window.set_cursor_grab(CursorGrabMode::Locked).is_ok()
                                || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
                            if self.captured {
                                window.set_cursor_visible(false);
                            }
                        }
                    }
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        renderer.resize(renderer.size)
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                    Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => {}
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.captured {
                self.camera.apply_mouse_motion(
                    delta.0,
                    delta.1,
                    self.app_settings.controls.mouse_sensitivity,
                );
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn persist_settings(settings: &mut settings::AppSettings, renderer: &renderer::Renderer) {
    settings.graphics = renderer.graphics.clone();
    if let Err(error) = settings.save(settings::SETTINGS_FILE) {
        eprintln!("Could not save {}: {error}", settings::SETTINGS_FILE);
    }
}

fn weapon_slot(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Digit1 => Some(0),
        KeyCode::Digit2 => Some(1),
        KeyCode::Digit3 => Some(2),
        KeyCode::Digit4 => Some(3),
        KeyCode::Digit5 => Some(4),
        KeyCode::Digit6 => Some(5),
        KeyCode::Digit7 => Some(6),
        KeyCode::Digit8 => Some(7),
        KeyCode::Digit9 => Some(8),
        _ => None,
    }
}
