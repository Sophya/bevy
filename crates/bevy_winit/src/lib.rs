//! `bevy_winit` provides utilities to handle window creation and the eventloop through [`winit`]
//!
//! Most commonly, the [`WinitPlugin`] is used as part of
//! [`DefaultPlugins`](https://docs.rs/bevy/latest/bevy/struct.DefaultPlugins.html).
//! The app's [runner](bevy_app::App::runner) is set by `WinitPlugin` and handles the `winit` [`EventLoop`].
//! See `winit_runner` for details.

pub mod accessibility;
mod converters;
// mod runner;
mod state;
mod system;
mod winit_config;
mod winit_windows;

use std::marker::PhantomData;
use bevy_a11y::AccessibilityRequested;
pub use system::create_windows;
use system::{changed_windows, despawn_windows};
pub use winit_config::*;
pub use winit_windows::*;

use bevy_app::{App, Last, Plugin};
use bevy_ecs::prelude::*;
use bevy_window::{exit_on_all_closed, Window, WindowCreated, WindowResized};
#[cfg(target_os = "android")]
use bevy_window::{PrimaryWindow, RawHandleWrapper};

#[cfg(target_os = "android")]
pub use winit::platform::android::activity as android_activity;

use winit::event_loop::EventLoopBuilder;

use crate::accessibility::{AccessKitAdapters, AccessKitPlugin, WinitActionHandlers};

use crate::state::winit_runner;

/// [`AndroidApp`] provides an interface to query the application state as well as monitor events
/// (for example lifecycle and input events).
#[cfg(target_os = "android")]
pub static ANDROID_APP: std::sync::OnceLock<android_activity::AndroidApp> =
    std::sync::OnceLock::new();

/// A [`Plugin`] that uses `winit` to create and manage windows, and receive window and input
/// events.
///
/// This plugin will add systems and resources that sync with the `winit` backend and also
/// replace the existing [`App`] runner with one that constructs an [event loop](EventLoop) to
/// receive window and input events from the OS.
#[derive(Default)]
pub struct WinitPlugin<T: Event> {
    /// Allows the window (and the event loop) to be created on any thread
    /// instead of only the main thread.
    ///
    /// See [`EventLoopBuilder::build`] for more information on this.
    ///
    /// # Supported platforms
    ///
    /// Only works on Linux (X11/Wayland) and Windows.
    /// This field is ignored on other platforms.
    pub run_on_any_thread: bool,
    _marker: PhantomData<T>,
}

impl<T: Event> Plugin for WinitPlugin<T> {
    fn build(&self, app: &mut App) {
        let mut event_loop_builder = EventLoopBuilder::<T>::with_user_event();

        // linux check is needed because x11 might be enabled on other platforms.
        #[cfg(all(target_os = "linux", feature = "x11"))]
        {
            use winit::platform::x11::EventLoopBuilderExtX11;

            // This allows a Bevy app to be started and ran outside of the main thread.
            // A use case for this is to allow external applications to spawn a thread
            // which runs a Bevy app without requiring the Bevy app to need to reside on
            // the main thread, which can be problematic.
            event_loop_builder.with_any_thread(self.run_on_any_thread);
        }

        // linux check is needed because wayland might be enabled on other platforms.
        #[cfg(all(target_os = "linux", feature = "wayland"))]
        {
            use winit::platform::wayland::EventLoopBuilderExtWayland;
            event_loop_builder.with_any_thread(self.run_on_any_thread);
        }

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            event_loop_builder.with_any_thread(self.run_on_any_thread);
        }

        #[cfg(target_os = "android")]
        {
            use winit::platform::android::EventLoopBuilderExtAndroid;
            let msg = "Bevy must be setup with the #[bevy_main] macro on Android";
            event_loop_builder.with_android_app(ANDROID_APP.get().expect(msg).clone());
        }

        app.add_event::<T>()
            .init_non_send_resource::<WinitWindows>()
            .init_resource::<WinitSettings>()
            .set_runner(winit_runner::<T>)
            .add_systems(
                Last,
                (
                    // `exit_on_all_closed` only checks if windows exist but doesn't access data,
                    // so we don't need to care about its ordering relative to `changed_windows`
                    changed_windows.ambiguous_with(exit_on_all_closed),
                    despawn_windows,
                )
                    .chain(),
            );

        app.add_plugins(AccessKitPlugin);

        let event_loop = event_loop_builder
            .build()
            .expect("Failed to build event loop");

        // iOS, macOS, and Android don't like it if you create windows before the event loop is
        // initialized.
        //
        // See:
        // - https://github.com/rust-windowing/winit/blob/master/README.md#macos
        // - https://github.com/rust-windowing/winit/blob/master/README.md#ios
        #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
        {
            // Otherwise, we want to create a window before `bevy_render` initializes the renderer
            // so that we have a surface to use as a hint. This improves compatibility with `wgpu`
            // backends, especially WASM/WebGL2.
            let mut create_window = SystemState::<CreateWindowParams>::from_world(&mut app.world);
            create_windows(&event_loop, create_window.get_mut(&mut app.world));
            create_window.apply(&mut app.world);
        }

        // `winit`'s windows are bound to the event loop that created them, so the event loop must
        // be inserted as a resource here to pass it onto the runner.
        app.insert_non_send_resource(event_loop);
    }
}

/// The default event that can be used to wake the window loop
/// Wakes up the loop if in wait state
#[derive(Debug, Default, Clone, Copy, Event)]
pub struct WakeUp;

/// The [`winit::event_loop::EventLoopProxy`] with the specific [`T`] used in the [`winit_runner`].
///
/// The `EventLoopProxy` can be used to request a redraw from outside bevy.
///
/// Use `NonSend<EventLoopProxy>` to receive this resource.
pub type EventLoopProxy<T> = winit::event_loop::EventLoopProxy<T>;

trait AppSendEvent {
    fn send_event<E: bevy_ecs::event::Event>(&mut self, event: E);
}

impl AppSendEvent for App {
    fn send_event<E: bevy_ecs::event::Event>(&mut self, event: E) {
        self.world.send_event(event);
    }
}

/// The parameters of the [`create_windows`] system.
pub type CreateWindowParams<'w, 's, F = ()> = (
    Commands<'w, 's>,
    Query<'w, 's, (Entity, &'static mut Window), F>,
    EventWriter<'w, WindowCreated>,
    NonSendMut<'w, WinitWindows>,
    NonSendMut<'w, AccessKitAdapters>,
    ResMut<'w, WinitActionHandlers>,
    Res<'w, AccessibilityRequested>,
);

fn react_to_resize(
    win: &mut Mut<'_, Window>,
    size: winit::dpi::PhysicalSize<u32>,
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
