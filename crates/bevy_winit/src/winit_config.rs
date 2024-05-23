use bevy_ecs::system::Resource;
use bevy_utils::Duration;

/// Settings for the [`WinitPlugin`](super::WinitPlugin).
#[derive(Debug, Resource, Clone, PartialEq)]
pub struct WinitSettings {
    /// Determines how frequently the application can update when it has focus.
    pub focused_mode: UpdateMode,
    /// Determines how frequently the application can update when it's out of focus.
    pub unfocused_mode: UpdateMode,
}

impl WinitSettings {
    /// Default settings for games.
    ///
    /// [`Continuous`](UpdateMode::continuous) if windows have focus,
    /// [`ReactiveLowPower`](UpdateMode::reactive_low_power) otherwise, with 60 updates every second.
    pub fn game() -> Self {
        WinitSettings {
            focused_mode: UpdateMode::continuous(),
            unfocused_mode: UpdateMode::reactive_at_target_interval(Duration::from_secs_f64(
                1.0 / 60.0,
            )),
        }
    }

    /// Default settings for desktop applications.
    ///
    /// [`Reactive`](UpdateMode::reactive) if windows have focus, with an update every 1 second.
    /// [`ReactiveLowPower`](UpdateMode::reactive_low_power) otherwise, with an update every 60 seconds.
    pub fn desktop_app() -> Self {
        WinitSettings {
            focused_mode: UpdateMode::reactive_at_target_interval(Duration::from_secs(1)),
            unfocused_mode: UpdateMode::reactive_low_power_at_target_interval(Duration::from_secs(
                60,
            )),
        }
    }

    /// Returns the current [`UpdateMode`].
    ///
    /// **Note:** The output depends on whether the window has focus or not.
    pub fn update_mode(&self, focused: bool) -> UpdateMode {
        match focused {
            true => self.focused_mode,
            false => self.unfocused_mode,
        }
    }
}

impl Default for WinitSettings {
    fn default() -> Self {
        WinitSettings::game()
    }
}

/// Represents how the application should update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UpdateMode {
    /// Determines what events should trigger an [`App`](bevy_app::App) update.
    /// Additionally, the app will also be updated if:
    ///   - A `wait` time has elapsed since the previous update
    ///   - A redraw has been requested by [`RequestRedraw`](bevy_window::RequestRedraw)
    pub reactivity: Reactivity,
    /// Determines how frequently an [`App`](bevy_app::App) should update.
    ///
    /// **Note:** This setting is independent of VSync. VSync is controlled by a window's
    /// [`PresentMode`](bevy_window::PresentMode) setting. If an app can update faster than the refresh
    /// rate, but VSync is enabled, the update rate will be indirectly limited by the renderer.
    pub update_frequency: UpdateFrequency,
}

impl UpdateMode {
    /// Creates an `UpdateMode` with continuous updates.
    ///
    /// The application will update as frequently as possible.
    pub fn continuous() -> Self {
        Self {
            reactivity: Reactivity::reactive(),
            update_frequency: UpdateFrequency::Continuous,
        }
    }

    /// Creates an `UpdateMode` with reactive updates and a specified target interval.
    ///
    /// The application will update reactively and wait for the specified interval between updates.
    ///
    /// # Arguments
    ///
    /// * `interval` - The duration to wait between updates.
    pub fn reactive_at_target_interval(interval: Duration) -> Self {
        Self {
            reactivity: Reactivity::reactive(),
            update_frequency: UpdateFrequency::TargetInterval(interval),
        }
    }

    /// Creates an `UpdateMode` with low-power reactive updates and a specified target interval.
    ///
    /// The application will update reactively with low power consumption and wait for the specified interval between updates.
    ///
    /// # Arguments
    ///
    /// * `interval` - The duration to wait between updates.
    pub fn reactive_low_power_at_target_interval(interval: Duration) -> Self {
        Self {
            reactivity: Reactivity::reactive_low_power(),
            update_frequency: UpdateFrequency::TargetInterval(interval),
        }
    }
}

/// Determines how frequently an [`App`](bevy_app::App) should update.
///
/// **Note:** This setting is independent of VSync. VSync is controlled by a window's
/// [`PresentMode`](bevy_window::PresentMode) setting. If an app can update faster than the refresh
/// rate, but VSync is enabled, the update rate will be indirectly limited by the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UpdateFrequency {
    /// The [`App`](bevy_app::App) will update over and over, as fast as it possibly can, until an
    /// [`AppExit`](bevy_app::AppExit) event appears.
    Continuous,
    /// The approximate time from the start of one update to the next.
    ///
    /// **Note:** This has no upper limit.
    /// The [`App`](bevy_app::App) will wait indefinitely if you set this to [`Duration::MAX`].
    TargetInterval(Duration),
}

/// Determines what events should trigger an [`App`](bevy_app::App) update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reactivity {
    /// Indicates whether the application should react to window events.
    pub react_to_window_events: bool,
    /// Indicates whether the application should react to device events.
    pub react_to_device_events: bool,
    /// Indicates whether the application should react to user events sent through the [`EventLoopProxy`](crate::EventLoopProxy)
    pub react_to_user_events: bool,
}

impl Reactivity {
    /// React to window, device and user events
    pub fn reactive() -> Self {
        Self {
            react_to_window_events: true,
            react_to_device_events: true,
            react_to_user_events: true,
        }
    }

    /// React to window and user events, but not to device events
    pub fn reactive_low_power() -> Self {
        Self {
            react_to_window_events: true,
            react_to_device_events: false,
            react_to_user_events: true,
        }
    }

    /// React only to user events, but not to window or device events
    pub fn manual() -> Self {
        Self {
            react_to_window_events: false,
            react_to_device_events: false,
            react_to_user_events: true,
        }
    }
}
