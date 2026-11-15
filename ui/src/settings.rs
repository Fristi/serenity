//! Persistence port for the user's customize settings.
//!
//! `Settings` is the struct generated from `export struct Settings` in
//! `app-window.slint`, so there is no conversion layer between the UI and the
//! host. Hosts that cannot persist (both of today's) use [`NoopStore`].

pub use crate::Settings;

/// Loads settings at startup and receives every change.
///
/// `save` runs on each UI change rather than on Back, which is fine for
/// localStorage. A flash-backed store should skip writes whose value equals the
/// last one it wrote: `App` applies the loaded settings after wiring the
/// callback, so startup produces one save that changes nothing.
pub trait SettingsStore {
    /// `None` means "nothing stored"; the UI keeps its defaults.
    fn load(&mut self) -> Option<Settings>;
    fn save(&mut self, settings: Settings);
}

/// Discards writes and never restores anything.
pub struct NoopStore;

impl SettingsStore for NoopStore {
    fn load(&mut self) -> Option<Settings> {
        None
    }

    fn save(&mut self, _settings: Settings) {}
}

impl SettingsStore for () {
    fn load(&mut self) -> Option<Settings> {
        None
    }

    fn save(&mut self, _settings: Settings) {}
}
