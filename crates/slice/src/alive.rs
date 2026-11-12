//! Async callbacks must not write into an owner disposed by navigation.
use leptos::prelude::*;
pub(crate) trait AliveSignal<T> {
    fn set_if_alive(&self, value: T);
    fn update_if_alive(&self, update: impl FnOnce(&mut T));
}
impl<T: Send + Sync + 'static> AliveSignal<T> for RwSignal<T> {
    fn set_if_alive(&self, value: T) {
        let _ = self.try_set(value);
    }
    fn update_if_alive(&self, update: impl FnOnce(&mut T)) {
        let _ = self.try_update(update);
    }
}
impl<T: Send + Sync + 'static> AliveSignal<T> for WriteSignal<T> {
    fn set_if_alive(&self, value: T) {
        let _ = self.try_set(value);
    }
    fn update_if_alive(&self, update: impl FnOnce(&mut T)) {
        let _ = self.try_update(update);
    }
}
