//! `bevy_winit` provides utilities to handle window creation and the eventloop through [`winit`]
//!
//! Most commonly, the [`WinitPlugin`] is used as part of
//! [`DefaultPlugins`](https://docs.rs/bevy/latest/bevy/struct.DefaultPlugins.html).
//! The app's [runner](bevy_app::App::runner) is set by `WinitPlugin` and handles the `winit` [`EventLoop`].
//! See `winit_runner` for details.

use approx::relative_eq;
use bevy_utils::Instant;
use winit::dpi::{LogicalSize, PhysicalSize};

use bevy_app::{App, AppExit, PluginsState};
use bevy_ecs::event::{Events, ManualEventReader};
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemState;
use bevy_input::{
    mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
    touchpad::{TouchpadMagnify, TouchpadRotate},
};
use bevy_math::{ivec2, DVec2, Vec2};
#[cfg(not(target_arch = "wasm32"))]
use bevy_tasks::tick_global_task_pools_on_main_thread;
use bevy_utils::tracing::{error, trace, warn};
use bevy_window::{
    ApplicationLifetime, CursorEntered, CursorLeft, CursorMoved, FileDragAndDrop, Ime,
    ReceivedCharacter, RequestRedraw, Window, WindowBackendScaleFactorChanged,
    WindowCloseRequested, WindowDestroyed, WindowFocused, WindowMoved, WindowOccluded,
    WindowResized, WindowScaleFactorChanged, WindowThemeChanged,
};
#[cfg(target_os = "android")]
use bevy_window::{PrimaryWindow, RawHandleWrapper};

#[cfg(target_os = "android")]
pub use winit::platform::android::activity as android_activity;

use winit::event::StartCause;
use winit::{
    event::{self, DeviceEvent, Event as WinitEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
};

use crate::accessibility::AccessKitAdapters;

use crate::converters::convert_winit_theme;
use crate::system::CachedWindow;
use crate::{
    converters, create_windows, AppSendEvent, CreateWindowParams, UpdateMode, WinitEventFilter,
    WinitSettings, WinitWindows,
};

/// [`AndroidApp`] provides an interface to query the application state as well as monitor events
/// (for example lifecycle and input events).
#[cfg(target_os = "android")]
pub static ANDROID_APP: std::sync::OnceLock<android_activity::AndroidApp> =
    std::sync::OnceLock::new();

/// Persistent state that is used to run the [`App`] according to the current
/// [`UpdateMode`].
struct WinitAppRunnerState<T: Event> {
    /// Current activity state of the app.
    activity_state: UpdateState,
    /// Current update mode of the app.
    update_mode: UpdateMode,
    /// Filter to handle events
    event_filter: WinitEventFilter<T>,
    /// Number of "forced" updates to trigger on application start
    startup_forced_updates: u32,
    /// Is `true` if the app has requested a redraw since the last update.
    redraw_requested: bool,
    /// Is `true` if enough time has elapsed since `last_update` to run another update.
    wait_elapsed: bool,
    /// Is `true` if a filtered event has been received since the last update.
    event_received: bool,
}

impl<T: Event> WinitAppRunnerState<T> {
    fn reset_on_update(&mut self) {
        self.event_received;
    }

    fn should_update(&self) -> bool {
        (self.wait_elapsed || self.event_received) && self.activity_state.is_active()
    }

    fn handle_event(&mut self, event: &WinitEvent<T>, update_mode: UpdateMode) {
        self.event_received |= self.event_filter.handle(event, update_mode);
    }
}

impl<T: Event> Default for WinitAppRunnerState<T> {
    fn default() -> Self {
        Self {
            activity_state: UpdateState::NotYetStarted,
            update_mode: UpdateMode::Continuous,
            redraw_requested: false,
            wait_elapsed: false,
            // 3 seems to be enough, 5 is a safe margin
            startup_forced_updates: 5,
            event_filter: WinitEventFilter::default(),
            event_received: false,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
enum UpdateState {
    NotYetStarted,
    Active,
    Suspended,
    WillSuspend,
    WillResume,
}

impl UpdateState {
    #[inline]
    fn is_active(&self) -> bool {
        match self {
            Self::NotYetStarted | Self::Suspended => false,
            Self::Active | Self::WillSuspend | Self::WillResume => true,
        }
    }
}

/// The default [`App::runner`] for the [`WinitPlugin`] plugin.
///
/// Overriding the app's [runner](bevy_app::App::runner) while using `WinitPlugin` will bypass the
/// `EventLoop`.
pub fn winit_runner<T: Event>(mut app: App) {
    if app.plugins_state() == PluginsState::Ready {
        app.finish();
        app.cleanup();
    }

    let event_loop = app
        .world
        .remove_non_send_resource::<EventLoop<T>>()
        .unwrap();

    app.world
        .insert_non_send_resource(event_loop.create_proxy());

    let mut runner_state = WinitAppRunnerState::<T>::default();

    if let Some(filter) = app.world.remove_non_send_resource::<WinitEventFilter<T>>() {
        runner_state.event_filter = filter;
    }

    // prepare structures to access data in the world
    let mut app_exit_event_reader = ManualEventReader::<AppExit>::default();
    let mut redraw_event_reader = ManualEventReader::<RequestRedraw>::default();
    let mut window_backend_scale_factor_changed_reader =
        ManualEventReader::<WindowBackendScaleFactorChanged>::default();

    let mut focused_windows_state: SystemState<(Res<WinitSettings>, Query<(Entity, &Window)>)> =
        SystemState::new(&mut app.world);

    let mut event_writer_system_state: SystemState<(
        EventWriter<WindowResized>,
        NonSend<WinitWindows>,
        Query<(&mut Window, &mut CachedWindow)>,
        NonSend<AccessKitAdapters>,
    )> = SystemState::new(&mut app.world);

    let mut create_window =
        SystemState::<CreateWindowParams<Added<Window>>>::from_world(&mut app.world);
    // set up the event loop
    let event_handler = move |event, event_loop: &EventLoopWindowTarget<T>| {
        handle_winit_event(
            &mut app,
            &mut app_exit_event_reader,
            &mut runner_state,
            &mut create_window,
            &mut event_writer_system_state,
            &mut focused_windows_state,
            &mut redraw_event_reader,
            &mut window_backend_scale_factor_changed_reader,
            event,
            event_loop,
        );
    };

    trace!("starting winit event loop");
    // TODO(clean): the winit docs mention using `spawn` instead of `run` on WASM.
    if let Err(err) = event_loop.run(event_handler) {
        error!("winit event loop returned an error: {err}");
    }
}

#[allow(clippy::too_many_arguments /* TODO: probs can reduce # of args */)]
fn handle_winit_event<T: Event>(
    app: &mut App,
    app_exit_event_reader: &mut ManualEventReader<AppExit>,
    runner_state: &mut WinitAppRunnerState<T>,
    create_window: &mut SystemState<CreateWindowParams<Added<Window>>>,
    event_writer_system_state: &mut SystemState<(
        EventWriter<WindowResized>,
        NonSend<WinitWindows>,
        Query<(&mut Window, &mut CachedWindow)>,
        NonSend<AccessKitAdapters>,
    )>,
    focused_windows_state: &mut SystemState<(Res<WinitSettings>, Query<(Entity, &Window)>)>,
    redraw_event_reader: &mut ManualEventReader<RequestRedraw>,
    window_backend_scale_factor_changed_event_reader: &mut ManualEventReader<
        WindowBackendScaleFactorChanged,
    >,
    event: WinitEvent<T>,
    event_loop: &EventLoopWindowTarget<T>,
) {
    #[cfg(feature = "trace")]
    let _span = bevy_utils::tracing::info_span!("winit event_handler").entered();

    if app.plugins_state() != PluginsState::Cleaned {
        if app.plugins_state() != PluginsState::Ready {
            #[cfg(not(target_arch = "wasm32"))]
            tick_global_task_pools_on_main_thread();
        } else {
            app.finish();
            app.cleanup();
        }
        runner_state.redraw_requested = true;
    }

    // create any new windows
    // (even if app did not update, some may have been created by plugin setup)
    create_windows(event_loop, create_window.get_mut(&mut app.world));
    create_window.apply(&mut app.world);

    let (config, windows) = focused_windows_state.get(&app.world);
    let focused = windows.iter().any(|(_, window)| window.focused);
    let mut update_mode = config.update_mode(focused);

    runner_state.handle_event(&event, update_mode);

    #[cfg(target_arch = "wasm32")]
    {
        use bevy_window::WindowGlContextLost;
        use wasm_bindgen::JsCast;
        use winit::platform::web::WindowExtWebSys;

        fn get_gl_context(
            window: &winit::window::Window,
        ) -> Option<web_sys::WebGl2RenderingContext> {
            if let Some(canvas) = window.canvas() {
                let context = canvas.get_context("webgl2").ok()??;

                Some(context.dyn_into::<web_sys::WebGl2RenderingContext>().ok()?)
            } else {
                None
            }
        }

        fn has_gl_context(window: &winit::window::Window) -> bool {
            get_gl_context(window).map_or(false, |ctx| !ctx.is_context_lost())
        }

        let (_, windows) = focused_windows_state.get(&app.world);

        if let Some((entity, _)) = windows.iter().next() {
            let winit_windows = app.world.non_send_resource::<WinitWindows>();
            let window = winit_windows.get_window(entity).expect("Window must exist");

            if !has_gl_context(&window) {
                app.world.send_event(WindowGlContextLost { window: entity });

                // Pauses sub-apps to stop WGPU from crashing when there's no OpenGL context.
                // Ensures that the rest of the systems in the main app keep running (i.e. physics).
                app.pause_sub_apps();
            } else {
                app.resume_sub_apps();
            }
        }
    }

    match event {
        WinitEvent::AboutToWait => {
            if let Some(redraw_events) = app.world.get_resource::<Events<RequestRedraw>>() {
                if redraw_event_reader.read(redraw_events).last().is_some() {
                    runner_state.redraw_requested = true;
                }
            }

            if let Some(backend_scale_factor_changed_events) = app
                .world
                .get_resource::<Events<WindowBackendScaleFactorChanged>>()
            {
                let events = window_backend_scale_factor_changed_event_reader
                    .read(backend_scale_factor_changed_events)
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(evt) = events.last() {
                    let (mut window_resized, winit_windows, mut windows, _) =
                        event_writer_system_state.get_mut(&mut app.world);

                    let Some(winit_window) = winit_windows.get_window(evt.window) else {
                        warn!("Unknown winit window Id {:?}", evt.window);
                        return;
                    };

                    let Ok((mut window, _)) = windows.get_mut(evt.window) else {
                        warn!("Window {:?} is missing `Window` component", evt.window);
                        return;
                    };

                    if let Some(logical_size) = react_to_scale_factor_changed(
                        &mut window,
                        winit_window,
                        evt.scale_factor as f32,
                    ) {
                        window_resized.send(WindowResized {
                            window: evt.window,
                            width: logical_size.width,
                            height: logical_size.height,
                        });
                    }
                }
            }

            let mut should_update = runner_state.should_update();

            if runner_state.startup_forced_updates > 0 {
                runner_state.startup_forced_updates -= 1;
                // Ensure that an update is triggered on the first iterations for app initialization
                should_update = true;
            }

            if runner_state.activity_state == UpdateState::WillSuspend {
                runner_state.activity_state = UpdateState::Suspended;
                // Trigger one last update to enter the suspended state
                should_update = true;

                #[cfg(target_os = "android")]
                {
                    // Remove the `RawHandleWrapper` from the primary window.
                    // This will trigger the surface destruction.
                    let mut query = app.world.query_filtered::<Entity, With<PrimaryWindow>>();
                    let entity = query.single(&app.world);
                    app.world.entity_mut(entity).remove::<RawHandleWrapper>();
                }
            }

            if runner_state.activity_state == UpdateState::WillResume {
                runner_state.activity_state = UpdateState::Active;
                // Trigger the update to enter the active state
                should_update = true;
                // Trigger the next redraw ro refresh the screen immediately
                runner_state.redraw_requested = true;

                #[cfg(target_os = "android")]
                {
                    // Get windows that are cached but without raw handles. Those window were already created, but got their
                    // handle wrapper removed when the app was suspended.
                    let mut query = app
                        .world
                        .query_filtered::<(Entity, &Window), (With<CachedWindow>, Without<bevy_window::RawHandleWrapper>)>();
                    if let Ok((entity, window)) = query.get_single(&app.world) {
                        use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
                        let window = window.clone();

                        let (
                            ..,
                            mut winit_windows,
                            mut adapters,
                            mut handlers,
                            accessibility_requested,
                        ) = create_window.get_mut(&mut app.world);

                        let winit_window = winit_windows.create_window(
                            event_loop,
                            entity,
                            &window,
                            &mut adapters,
                            &mut handlers,
                            &accessibility_requested,
                        );

                        let wrapper = RawHandleWrapper {
                            window_handle: winit_window.window_handle().unwrap().as_raw(),
                            display_handle: winit_window.display_handle().unwrap().as_raw(),
                        };

                        app.world.entity_mut(entity).insert(wrapper);
                    }
                }
            }

            // This is recorded before running app.update(), to run the next cycle after a correct timeout.
            // If the cycle takes more than the wait timeout, it will be re-executed immediately.
            let begin_frame_time = Instant::now();

            if should_update {
                // Not redrawing, but the timeout elapsed.
                run_app_update(runner_state, app);

                // Running the app may have changed the WinitSettings resource, so we have to re-extract it.
                let (config, windows) = focused_windows_state.get(&app.world);
                let focused = windows.iter().any(|(_, window)| window.focused);
                update_mode = config.update_mode(focused);
            }

            if update_mode != runner_state.update_mode {
                // Trigger the next redraw since we're changing the update mode
                runner_state.redraw_requested = true;
                // Consider the wait as elapsed since it could have been cancelled by a user event
                runner_state.wait_elapsed = true;

                runner_state.update_mode = update_mode;
            }

            match update_mode {
                UpdateMode::Continuous => {
                    // per winit's docs on [Window::is_visible](https://docs.rs/winit/latest/winit/window/struct.Window.html#method.is_visible),
                    // we cannot use the visibility to drive rendering on these platforms
                    // so we cannot discern whether to beneficially use `Poll` or not?
                    cfg_if::cfg_if! {
                        if #[cfg(not(any(
                            target_arch = "wasm32",
                            target_os = "android",
                            target_os = "ios",
                            all(target_os = "linux", any(feature = "x11", feature = "wayland"))
                        )))]
                        {
                            let winit_windows = app.world.non_send_resource::<WinitWindows>();
                            let visible = winit_windows.windows.iter().any(|(_, w)| {
                                w.is_visible().unwrap_or(false)
                            });

                            event_loop.set_control_flow(if visible {
                                ControlFlow::Wait
                            } else {
                                ControlFlow::Poll
                            });
                        }
                        else {
                            event_loop.set_control_flow(ControlFlow::Wait);
                        }
                    }

                    // Trigger the next redraw to refresh the screen immediately if waiting
                    if let ControlFlow::Wait = event_loop.control_flow() {
                        runner_state.redraw_requested = true;
                    }
                }
                UpdateMode::Reactive { wait, .. } => {
                    // Set the next timeout, starting from the instant before running app.update() to avoid frame delays
                    if let Some(next) = begin_frame_time.checked_add(wait) {
                        if runner_state.wait_elapsed {
                            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
                        }
                    }
                }
            }

            if runner_state.redraw_requested
                && runner_state.activity_state != UpdateState::Suspended
            {
                let (_, winit_windows, _, _) = event_writer_system_state.get_mut(&mut app.world);
                for window in winit_windows.windows.values() {
                    window.request_redraw();
                }
                runner_state.redraw_requested = false;
            }
        }
        WinitEvent::NewEvents(cause) => {
            runner_state.wait_elapsed = match cause {
                StartCause::WaitCancelled {
                    requested_resume: Some(_),
                    ..
                } => false,
                _ => true,
            };
        }
        WinitEvent::WindowEvent {
            event, window_id, ..
        } => {
            let (mut window_resized, winit_windows, mut windows, access_kit_adapters) =
                event_writer_system_state.get_mut(&mut app.world);

            let Some(window) = winit_windows.get_window_entity(window_id) else {
                warn!("Skipped event {event:?} for unknown winit window Id {window_id:?}");
                return;
            };

            let Ok((mut win, _)) = windows.get_mut(window) else {
                warn!("Window {window:?} is missing `Window` component, skipping event {event:?}");
                return;
            };

            // Allow AccessKit to respond to `WindowEvent`s before they reach
            // the engine.
            if let Some(adapter) = access_kit_adapters.get(&window) {
                if let Some(winit_window) = winit_windows.get_window(window) {
                    adapter.process_event(winit_window, &event);
                }
            }

            match event {
                WindowEvent::Resized(size) => {
                    react_to_resize(&mut win, size, &mut window_resized, window);
                }
                WindowEvent::CloseRequested => app.send_event(WindowCloseRequested { window }),
                WindowEvent::KeyboardInput { ref event, .. } => {
                    if event.state.is_pressed() {
                        if let Some(char) = &event.text {
                            let char = char.clone();
                            app.send_event(ReceivedCharacter { window, char });
                        }
                    }
                    app.send_event(converters::convert_keyboard_input(event, window));
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let physical_position = DVec2::new(position.x, position.y);

                    let last_position = win.physical_cursor_position();
                    let delta = last_position.map(|last_pos| {
                        (physical_position.as_vec2() - last_pos) / win.resolution.scale_factor()
                    });

                    win.set_physical_cursor_position(Some(physical_position));
                    let position =
                        (physical_position / win.resolution.scale_factor() as f64).as_vec2();
                    app.send_event(CursorMoved {
                        window,
                        position,
                        delta,
                    });
                }
                WindowEvent::CursorEntered { .. } => {
                    app.send_event(CursorEntered { window });
                }
                WindowEvent::CursorLeft { .. } => {
                    win.set_physical_cursor_position(None);
                    app.send_event(CursorLeft { window });
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    app.send_event(MouseButtonInput {
                        button: converters::convert_mouse_button(button),
                        state: converters::convert_element_state(state),
                        window,
                    });
                }
                WindowEvent::TouchpadMagnify { delta, .. } => {
                    app.send_event(TouchpadMagnify(delta as f32));
                }
                WindowEvent::TouchpadRotate { delta, .. } => {
                    app.send_event(TouchpadRotate(delta));
                }
                WindowEvent::MouseWheel { delta, .. } => match delta {
                    event::MouseScrollDelta::LineDelta(x, y) => {
                        app.send_event(MouseWheel {
                            unit: MouseScrollUnit::Line,
                            x,
                            y,
                            window,
                        });
                    }
                    event::MouseScrollDelta::PixelDelta(p) => {
                        app.send_event(MouseWheel {
                            unit: MouseScrollUnit::Pixel,
                            x: p.x as f32,
                            y: p.y as f32,
                            window,
                        });
                    }
                },
                WindowEvent::Touch(touch) => {
                    let location = touch
                        .location
                        .to_logical(win.resolution.scale_factor() as f64);
                    app.send_event(converters::convert_touch_input(touch, location, window));
                }
                WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                    let previous_factor = win.resolution.scale_factor();
                    let scale_factor_override = win.resolution.scale_factor_override();

                    app.send_event(WindowBackendScaleFactorChanged {
                        window,
                        scale_factor,
                    });

                    if scale_factor_override.is_none()
                        && !relative_eq!(scale_factor as f32, previous_factor)
                    {
                        app.send_event(WindowScaleFactorChanged {
                            window,
                            scale_factor,
                        });
                    }
                }
                WindowEvent::Focused(focused) => {
                    win.focused = focused;
                    app.send_event(WindowFocused { window, focused });
                }
                WindowEvent::Occluded(occluded) => {
                    app.send_event(WindowOccluded { window, occluded });
                }
                WindowEvent::DroppedFile(path_buf) => {
                    app.send_event(FileDragAndDrop::DroppedFile { window, path_buf });
                }
                WindowEvent::HoveredFile(path_buf) => {
                    app.send_event(FileDragAndDrop::HoveredFile { window, path_buf });
                }
                WindowEvent::HoveredFileCancelled => {
                    app.send_event(FileDragAndDrop::HoveredFileCanceled { window });
                }
                WindowEvent::Moved(position) => {
                    let position = ivec2(position.x, position.y);
                    win.position.set(position);
                    app.send_event(WindowMoved { window, position });
                }
                WindowEvent::Ime(event) => match event {
                    event::Ime::Preedit(value, cursor) => {
                        app.send_event(Ime::Preedit {
                            window,
                            value,
                            cursor,
                        });
                    }
                    event::Ime::Commit(value) => {
                        app.send_event(Ime::Commit { window, value });
                    }
                    event::Ime::Enabled => {
                        app.send_event(Ime::Enabled { window });
                    }
                    event::Ime::Disabled => {
                        app.send_event(Ime::Disabled { window });
                    }
                },
                WindowEvent::ThemeChanged(theme) => {
                    app.send_event(WindowThemeChanged {
                        window,
                        theme: convert_winit_theme(theme),
                    });
                }
                WindowEvent::Destroyed => {
                    app.send_event(WindowDestroyed { window });
                }
                _ => {}
            }

            let mut windows = app.world.query::<(&mut Window, &mut CachedWindow)>();
            if let Ok((window_component, mut cache)) = windows.get_mut(&mut app.world, window) {
                if window_component.is_changed() {
                    cache.window = window_component.clone();
                }
            }
        }
        WinitEvent::DeviceEvent { event, .. } => {
            if let DeviceEvent::MouseMotion { delta: (x, y) } = event {
                let delta = Vec2::new(x as f32, y as f32);
                app.send_event(MouseMotion { delta });
            }
        }
        WinitEvent::Suspended => {
            app.send_event(ApplicationLifetime::Suspended);
            // Mark the state as `WillSuspend`. This will let the schedule run one last time
            // before actually suspending to let the application react
            runner_state.activity_state = UpdateState::WillSuspend;
        }
        WinitEvent::Resumed => {
            match runner_state.activity_state {
                UpdateState::NotYetStarted => app.send_event(ApplicationLifetime::Started),
                _ => app.send_event(ApplicationLifetime::Resumed),
            }
            runner_state.activity_state = UpdateState::WillResume;
        }
        WinitEvent::UserEvent(event) => {
            app.world.send_event(event);
        }
        _ => (),
    }

    if let Some(app_exit_events) = app.world.get_resource::<Events<AppExit>>() {
        if app_exit_event_reader.read(app_exit_events).last().is_some() {
            event_loop.exit();
            return;
        }
    }
}

fn run_app_update<T: Event>(runner_state: &mut WinitAppRunnerState<T>, app: &mut App) {
    runner_state.reset_on_update();

    if app.plugins_state() == PluginsState::Cleaned {
        app.update();
    }
}

pub fn react_to_resize(
    win: &mut Mut<'_, Window>,
    size: PhysicalSize<u32>,
    window_resized: &mut EventWriter<WindowResized>,
    window: Entity,
) {
    win.resolution
        .set_physical_resolution(size.width, size.height);

    window_resized.send(WindowResized {
        window,
        width: win.width(),
        height: win.height(),
    });
}

pub fn react_to_scale_factor_changed(
    window: &mut Mut<'_, Window>,
    winit_window: &winit::window::Window,
    scale_factor: f32,
) -> Option<LogicalSize<f32>> {
    window.resolution.set_scale_factor(scale_factor);
    // Note: this may be different from new_scale_factor if
    // `scale_factor_override` is set to Some(thing)
    let mut new_factor = window.resolution.scale_factor();

    let mut new_physical_size =
        PhysicalSize::new(window.physical_width(), window.physical_height());

    if let Some(forced_factor) = window.resolution.scale_factor_override() {
        // This window is overriding the OS-suggested DPI, so its physical size
        // should be set based on the overriding value. Its logical size already
        // incorporates any resize constraints.
        new_physical_size = LogicalSize::new(window.width(), window.height())
            .to_physical::<u32>(forced_factor as f64);

        let _ = winit_window.request_inner_size(new_physical_size);
        new_factor = forced_factor;
    }

    window
        .resolution
        .set_physical_resolution(new_physical_size.width, new_physical_size.height);

    let new_logical_size = new_physical_size.to_logical(new_factor as f64);

    if !relative_eq!(window.width(), new_logical_size.width)
        || !relative_eq!(window.height(), new_logical_size.height)
    {
        return Some(new_logical_size);
    }

    None
}
