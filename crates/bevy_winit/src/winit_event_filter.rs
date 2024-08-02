use winit::event::Event as WinitEvent;

use bevy_ecs::prelude::*;

use crate::UpdateMode;

pub struct WinitEventFilter<T: Event> {
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
        let Some(filter) = &self.filter else {
            return false;
        };

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

        filter(event, update_mode)
    }
}