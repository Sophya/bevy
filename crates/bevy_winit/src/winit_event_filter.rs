use winit::event::Event as WinitEvent;

use bevy_ecs::prelude::*;

use crate::{UpdateMode, WakeUp};

pub struct WinitEventFilter<T: Event = WakeUp> {
    pub filter: Option<Box<dyn Fn(&WinitEvent<T>, UpdateMode) -> bool + Send + 'static>>,
}

impl<T: Event> Default for WinitEventFilter<T> {
    fn default() -> Self {
        Self { filter: None }
    }
}

impl<T: Event> WinitEventFilter<T> {
    pub fn new<F: Fn(&WinitEvent<T>, UpdateMode) -> bool + Send + 'static>(filter: F) -> Self {
        Self {
            filter: Some(Box::new(filter)),
        }
    }

    pub fn handle(&self, event: &WinitEvent<T>, update_mode: UpdateMode) -> bool {
        let can_react = match update_mode {
            UpdateMode::Continuous => true,
            UpdateMode::Reactive {
                react_to_device_events,
                react_to_user_events,
                react_to_window_events,
                ..
            } =>
                match event {
                    WinitEvent::DeviceEvent { .. } => react_to_device_events,
                    WinitEvent::UserEvent(_) => react_to_user_events,
                    WinitEvent::WindowEvent { .. } => react_to_window_events,
                    _ => false,
                }
        };

        if !can_react {
            return false;
        }

        let Some(filter) = &self.filter else {
            return true;
        };

        filter(event, update_mode)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use super::*;

    #[test]
    fn test_default() {
        let filter = WinitEventFilter::default();

        assert!(filter.handle(&WinitEvent::UserEvent(WakeUp), UpdateMode::Continuous));
    }

    #[test]
    fn test_filter() {
        let filter = WinitEventFilter::new(|e, _| match e {
            WinitEvent::UserEvent(e) => false,
            _ => true
        });

        assert!(!filter.handle(&WinitEvent::UserEvent(WakeUp), UpdateMode::Continuous));
    }

    #[test]
    fn test_update_mode() {
        let filter = WinitEventFilter::default();
        let update_mode = UpdateMode::Reactive {
            wait: Duration::MAX,
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        };

        assert!(!filter.handle(&WinitEvent::UserEvent(WakeUp), update_mode));
    }
}