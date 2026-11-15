//! The app object hosts construct: the compiled window plus the port wiring.

use alloc::rc::Rc;
use core::cell::RefCell;

use crate::settings::SettingsStore;
use crate::slint::ComponentHandle as _;
use crate::sound::SoundSink;
use crate::{MainWindow, Settings};

/// Owns the generated component and routes its callbacks to the host ports.
pub struct App {
    component: MainWindow,
    /// Shared with the callback handlers, which hold the only other `Rc`.
    /// Nothing else reads these, so they are never borrowed here.
    _sink: Rc<RefCell<dyn SoundSink>>,
    _store: Rc<RefCell<dyn SettingsStore>>,
}

impl App {
    /// Builds the window, then restores the stored settings if there are any.
    ///
    /// The Slint platform must already be installed; on the MCU that is the
    /// host's job, on the web winit installs itself.
    pub fn new(
        sink: impl SoundSink + 'static,
        store: impl SettingsStore + 'static,
    ) -> Result<Self, crate::slint::PlatformError> {
        let component = MainWindow::new()?;

        // `set_handler` requires `'static`, so the ports live behind an `Rc`.
        // Slint's single-threaded contract (and the `unsafe-single-threaded`
        // feature on the MCU) makes the `RefCell`s sound.
        let sink: Rc<RefCell<dyn SoundSink>> = Rc::new(RefCell::new(sink));
        let handler_sink = sink.clone();
        component.on_play_sound(move |sound| handler_sink.borrow_mut().play(sound));

        let store: Rc<RefCell<dyn SettingsStore>> = Rc::new(RefCell::new(store));
        let handler_store = store.clone();
        component.on_settings_changed(move |settings| handler_store.borrow_mut().save(settings));

        // Applied after the callback is wired, so a host sees the restored
        // values as well (one redundant save for a real store to skip).
        if let Some(settings) = store.borrow_mut().load() {
            apply(&component, settings);
        }

        Ok(Self {
            component,
            _sink: sink,
            _store: store,
        })
    }

    pub fn component(&self) -> &MainWindow {
        &self.component
    }

    pub fn show(&self) -> Result<(), crate::slint::PlatformError> {
        self.component.show()
    }

    /// Runs Slint's own event loop. Only the web host uses this; the firmware
    /// drives the loop by hand so it can poll touch and push pixels over QSPI.
    pub fn run(self) -> Result<(), crate::slint::PlatformError> {
        self.component.run()
    }
}

/// Pushes stored settings into the window.
///
/// The setters only mark the window dirty; the UI coalesces them into one
/// `settings-changed` on the next event loop turn, so the host sees the restored
/// values once and never an intermediate mix.
fn apply(component: &MainWindow, settings: Settings) {
    component.set_minutes(settings.minutes);
    component.set_inhale(settings.inhale);
    component.set_hold_inhale(settings.hold_inhale);
    component.set_exhale(settings.exhale);
    component.set_hold_exhale(settings.hold_exhale);
    component.set_scent_on(settings.scent_on);
}