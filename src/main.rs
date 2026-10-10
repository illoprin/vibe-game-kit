mod camera;
mod collision;
mod gpu;
mod map;
mod mesh;
mod model;
mod obj;
mod pipeline;
mod renderer;
mod scene;
mod settings;
mod targets;
mod texture;
mod types;
mod ui;

use std::{collections::HashSet, error::Error, sync::Arc, time::Instant};

use camera::Camera;
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut app_settings = settings::AppSettings::load(settings::SETTINGS_FILE);
    if let Some(map_argument) = std::env::args().nth(1) {
        app_settings.map_path = map_argument;
    }
    if app_settings.map_path.trim().is_empty() {
        app_settings.map_path = settings::AppSettings::default().map_path;
    }
    let map_file = app_settings.map_path.clone();
    let map = map::Map::load(&map_file)?;
    let built_scene = scene::build(&map)?;
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        event_loop.create_window(
            Window::default_attributes()
                .with_title("OGK • Concrete Yard")
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
                    WindowEvent::RedrawRequested => {
                        let now = Instant::now();
                        let dt = now.duration_since(previous_frame).as_secs_f32().min(0.05);
                        previous_frame = now;
                        if !paused {
                            collision::update_player(
                                &mut camera,
                                &pressed,
                                &renderer.colliders,
                                dt,
                            );
                        }
                        match renderer.render(
                            &camera,
                            &window,
                            paused,
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
