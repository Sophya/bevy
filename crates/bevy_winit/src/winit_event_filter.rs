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
        self.filter
            .as_ref()
            .map_or(false, |f| f(event, update_mode))
    }
}