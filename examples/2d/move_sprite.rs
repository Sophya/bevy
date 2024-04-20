//! Renders a 2D scene containing a single, moving sprite.

use std::sync::{Arc, Mutex};

use bevy::{
    prelude::*,
    window::RequestRedraw,
    winit::{EventLoopProxy, WinitSettings},
};
use once_cell::sync::Lazy;

use wasm_bindgen::prelude::*;

static EVENT_LOOP_PROXY: Lazy<Arc<Mutex<Option<EventLoopProxy>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

#[wasm_bindgen]
/// Triggers an app update
pub fn send_redraw_request() -> Result<(), JsValue> {
    let proxy = EVENT_LOOP_PROXY.lock().unwrap();
    if let Some(proxy) = &*proxy {
        proxy
            .send_event(RequestRedraw)
            .map_err(|_| JsValue::from_str("Failed to send redraw event"))
    } else {
        Err(JsValue::from_str("Event loop proxy not initialized"))
    }
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, sprite_movement)
        .insert_resource(WinitSettings {
            focused_mode: bevy::winit::UpdateMode::Manual,
            unfocused_mode: bevy::winit::UpdateMode::Manual,
        })
        .run();
}

#[derive(Component)]
enum Direction {
    Up,
    Down,
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    event_loop_proxy: NonSend<EventLoopProxy>,
) {
    commands.spawn(Camera2dBundle::default());
    commands.spawn((
        SpriteBundle {
            texture: asset_server.load("branding/icon.png"),
            transform: Transform::from_xyz(100., 0., 0.),
            ..default()
        },
        Direction::Up,
    ));

    *EVENT_LOOP_PROXY.lock().unwrap() = Some((*event_loop_proxy).clone());
}

/// The sprite is animated by changing its translation depending on the time that has passed since
/// the last frame.
fn sprite_movement(time: Res<Time>, mut sprite_position: Query<(&mut Direction, &mut Transform)>) {
    //> for some reason, even in very long frames, delta_time is limited to 0.25s
    //> maybe it's the engine protecting us from reeeally long frames? it should not. Weird.
    // it may be because of Fixed Update ?
    info!(">> delta time (s): {}", time.delta_seconds());
    for (mut logo, mut transform) in &mut sprite_position {
        match *logo {
            Direction::Up => transform.translation.y += 150. * time.delta_seconds(),
            Direction::Down => transform.translation.y -= 150. * time.delta_seconds(),
        }

        if transform.translation.y > 200. {
            *logo = Direction::Down;
        } else if transform.translation.y < -200. {
            *logo = Direction::Up;
        }
    }
}
