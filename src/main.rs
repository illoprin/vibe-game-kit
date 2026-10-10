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
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window},
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
                println!("Usage: webgpu-cube [game.toml] [--game <game.toml>] [--map <map file>]");
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
    let mut game_state = game::GameState::new(&game_config, &map)?;
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        event_loop.create_window(
            Window::default_attributes()
                .with_title(format!("{} • OGK", game_config.name))
                .with_inner_size(PhysicalSize::new(1280, 800)),
        )?,
    );
    let mut renderer = pollster::block_on(renderer::Renderer::new(
        window.clone(),
        &map,
        built_scene,
        app_settings.graphics.clone(),
    ))?;
    persist_settings(&mut app_settings, &renderer);
    let mut camera = Camera::new();
    let mut pressed = HashSet::new();
    let mut captured = false;
    let mut firing = false;
    let mut paused = false;
    let mut previous_frame = Instant::now();

    event_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::Poll);
        match event {
            Event::WindowEvent { window_id, event } if window_id == window.id() => {
                let _ = renderer.handle_ui_event(&window, &event);
                match event {
                    WindowEvent::CloseRequested => {
                        persist_settings(&mut app_settings, &renderer);
                        target.exit();
                    }
                    WindowEvent::Resized(size) => renderer.resize(size),
                    WindowEvent::KeyboardInput { event, .. } => {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if code == KeyCode::Escape
                                && event.state == ElementState::Pressed
                                && !event.repeat
                            {
                                paused = !paused;
                                pressed.clear();
                                firing = false;
                                if paused {
                                    let _ = window.set_cursor_grab(CursorGrabMode::None);
                                    window.set_cursor_visible(true);
                                    captured = false;
                                } else {
                                    captured = window
                                        .set_cursor_grab(CursorGrabMode::Locked)
                                        .is_ok()
                                        || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
                                    if captured {
                                        window.set_cursor_visible(false);
                                    }
                                }
                                persist_settings(&mut app_settings, &renderer);
                            } else if event.state == ElementState::Pressed {
                                if !paused && !event.repeat {
                                    if code == KeyCode::F1 {
                                        renderer.debug_enabled = !renderer.debug_enabled;
                                    }
                                    if captured {
                                        if let Some(slot) = weapon_slot(code) {
                                            game_state.select_slot(slot);
                                        }
                                    }
                                }
                                if !paused {
                                    pressed.insert(code);
                                }
                            } else {
                                pressed.remove(&code);
                            }
                        }
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } if !captured && !paused => {
                        captured = window.set_cursor_grab(CursorGrabMode::Locked).is_ok()
                            || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
                        if captured {
                            window.set_cursor_visible(false);
                        }
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } if captured && !paused => firing = true,
                    WindowEvent::MouseInput {
                        state: ElementState::Released,
                        button: MouseButton::Left,
                        ..
                    } => firing = false,
                    WindowEvent::MouseWheel { delta, .. } if captured && !paused => {
                        let amount = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y,
                            MouseScrollDelta::PixelDelta(position) => position.y as f32,
                        };
                        if amount.abs() > f32::EPSILON {
                            game_state.cycle_weapon(if amount > 0.0 { 1 } else { -1 });
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        let now = Instant::now();
                        let frame_dt = now.duration_since(previous_frame).as_secs_f32();
                        let dt = frame_dt.min(0.05);
                        previous_frame = now;
                        if !paused {
                            collision::update_player(
                                &mut camera,
                                &pressed,
                                &renderer.colliders,
                                dt,
                            );
                            game_state.update(
                                dt,
                                &mut camera,
                                &game_config,
                                firing,
                                &renderer.colliders,
                            );
                        }
                        match renderer.render(
                            &camera,
                            &game_config,
                            &game_state,
                            &window,
                            paused,
                            frame_dt,
                            &mut app_settings.controls,
                            &mut app_settings.map_path,
                        ) {
                            Ok((resume_requested, settings_changed)) => {
                                if settings_changed || resume_requested {
                                    persist_settings(&mut app_settings, &renderer);
                                }
                                if resume_requested {
                                    paused = false;
                                    captured = window
                                        .set_cursor_grab(CursorGrabMode::Locked)
                                        .is_ok()
                                        || window.set_cursor_grab(CursorGrabMode::Confined).is_ok();
                                    if captured {
                                        window.set_cursor_visible(false);
                                    }
                                }
                            }
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                renderer.resize(renderer.size)
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => target.exit(),
                            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::DeviceEvent {
                event: winit::event::DeviceEvent::MouseMotion { delta },
                ..
            } if captured => {
                camera.apply_mouse_motion(
                    delta.0,
                    delta.1,
                    app_settings.controls.mouse_sensitivity,
                );
            }
            Event::AboutToWait => window.request_redraw(),
            _ => {}
        }
    })?;
    Ok(())
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
