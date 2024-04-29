//! This example illustrates how to implement basic framerate limit through the Winit settings.
//! Additionally, for wasm, it shows how to force an external redraw, to wake up the app on demand.

use std::time::Duration;

use bevy::{
    prelude::*,
    winit::{UpdateMode, WinitSettings},
};

fn main() {
    App::new()
        .insert_resource(Framerate(30.0))
        .add_plugins(DefaultPlugins)
        .add_systems(
            Startup,
            (
                test_setup::setup,
                // Improvement to force an external redraw in-between frames, so that switching
                // from low FPS to high FPS happens instantly rather than at the next frame
                #[cfg(target_arch = "wasm32")]
                wasm::setup_external_redraw,
            ),
        )
        .add_systems(
            Update,
            (
                test_setup::switch_framerate,
                test_setup::rotate_cube,
                test_setup::update_text,
            ),
        )
        .add_systems(PostUpdate, update_winit)
        .run();
}

/// Target framerate of the app
#[derive(Resource, Debug, Deref, DerefMut)]
struct Framerate(pub f64);

/// Update winit based on the current `Framerate`
fn update_winit(framerate: Res<Framerate>, mut winit_config: ResMut<WinitSettings>) {
    let max_tick_rate = Duration::from_secs(60);
    *winit_config = WinitSettings {
        focused_mode: UpdateMode::Reactive {
            wait: Duration::from_secs_f64(framerate.recip()).min(max_tick_rate),
            react_to_window_events: false,
            react_to_device_events: false,
        },
        unfocused_mode: UpdateMode::Reactive {
            wait: Duration::from_secs_f64(framerate.recip()).min(max_tick_rate),
            react_to_window_events: false,
            react_to_device_events: false,
        },
    }
}

/// Everything in this module is for setting up and animating the scene, and is not important to the
/// demonstrated features.
pub(crate) mod test_setup {
    use crate::Framerate;
    use bevy::{prelude::*, window::RequestRedraw};

    /// Switch between framerates by pressing numeric keys
    pub(crate) fn switch_framerate(
        mut framerate: ResMut<Framerate>,
        mouse_button_input: Res<ButtonInput<KeyCode>>,
    ) {
        if mouse_button_input.just_pressed(KeyCode::Digit0) {
            *framerate = Framerate(0.01);
        } else if mouse_button_input.just_pressed(KeyCode::Digit1) {
            *framerate = Framerate(0.5);
        } else if mouse_button_input.just_pressed(KeyCode::Digit2) {
            *framerate = Framerate(1.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit3) {
            *framerate = Framerate(2.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit4) {
            *framerate = Framerate(5.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit5) {
            *framerate = Framerate(10.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit6) {
            *framerate = Framerate(30.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit7) {
            *framerate = Framerate(60.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit8) {
            *framerate = Framerate(120.0);
        } else if mouse_button_input.just_pressed(KeyCode::Digit9) {
            *framerate = Framerate(999.9);
        }
    }

    #[derive(Component)]
    pub(crate) struct Rotator;

    /// Rotate the cube to make it clear when the app is updating
    pub(crate) fn rotate_cube(
        time: Res<Time>,
        mut cube_transform: Query<&mut Transform, With<Rotator>>,
    ) {
        for mut transform in &mut cube_transform {
            transform.rotate_x(time.delta_seconds());
            transform.rotate_local_y(time.delta_seconds());
        }
    }

    #[derive(Component)]
    pub struct FramerateText;

    pub(crate) fn update_text(
        mut frame: Local<usize>,
        framerate: Res<Framerate>,
        mut query: Query<&mut Text, With<FramerateText>>,
    ) {
        *frame += 1;
        let mut text = query.single_mut();
        text.sections[1].value = format!("{} FPS", framerate.to_string());
        text.sections[3].value = frame.to_string();
    }

    /// Set up a scene with a cube and some text
    pub fn setup(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        mut materials: ResMut<Assets<StandardMaterial>>,
        mut event: EventWriter<RequestRedraw>,
    ) {
        commands.spawn((
            PbrBundle {
                mesh: meshes.add(Cuboid::new(0.5, 0.5, 0.5)),
                material: materials.add(Color::rgb(0.8, 0.7, 0.6)),
                ..default()
            },
            Rotator,
        ));

        commands.spawn(DirectionalLightBundle {
            transform: Transform::from_xyz(1.0, 1.0, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        });
        commands.spawn(Camera3dBundle {
            transform: Transform::from_xyz(-2.0, 2.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        });
        event.send(RequestRedraw);
        commands.spawn((
            TextBundle::from_sections([
                TextSection::new(
                    "Press num keys to change target FPS\n",
                    TextStyle {
                        font_size: 50.0,
                        ..default()
                    },
                ),
                TextSection::from_style(TextStyle {
                    font_size: 50.0,
                    color: Color::GREEN,
                    ..default()
                }),
                TextSection::new(
                    "\nFrame: ",
                    TextStyle {
                        font_size: 50.0,
                        color: Color::YELLOW,
                        ..default()
                    },
                ),
                TextSection::from_style(TextStyle {
                    font_size: 50.0,
                    color: Color::YELLOW,
                    ..default()
                }),
            ])
            .with_style(Style {
                align_self: AlignSelf::FlexStart,
                position_type: PositionType::Absolute,
                top: Val::Px(5.0),
                left: Val::Px(5.0),
                ..default()
            }),
            FramerateText,
        ));
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm {
    use std::sync::{Arc, Mutex};

    use bevy::{ecs::system::NonSend, window::RequestRedraw, winit::EventLoopProxy};
    use once_cell::sync::Lazy;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::KeyboardEvent;

    pub static EVENT_LOOP_PROXY: Lazy<Arc<Mutex<Option<EventLoopProxy>>>> =
        Lazy::new(|| Arc::new(Mutex::new(None)));

    #[wasm_bindgen]
    pub fn request_redraw() -> Result<(), String> {
        let proxy = EVENT_LOOP_PROXY.lock().unwrap();
        if let Some(proxy) = &*proxy {
            proxy
                .send_event(RequestRedraw)
                .map_err(|_| "Request redraw error: failed to send event".to_string())
        } else {
            Err("Request redraw error: event loop proxy not found".to_string())
        }
    }

    pub(crate) fn setup_external_redraw(event_loop_proxy: NonSend<EventLoopProxy>) {
        *EVENT_LOOP_PROXY.lock().unwrap() = Some((*event_loop_proxy).clone());

        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();

        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            let key = event.key();
            if key.len() == 1 && key.chars().next().map_or(false, |ch| ch.is_digit(10)) {
                request_redraw().unwrap();
            }
        }) as Box<dyn FnMut(KeyboardEvent)>);

        document
            .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();
    }
}
