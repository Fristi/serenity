//! Drives the real component headlessly and asserts the host ports fire when
//! they should: sound cues at each breathing phase, and a settings save on
//! every persisted change.
//!
//! Slint's published `i-slint-backend-testing` crate does not compile from crates.io
//! (it `include_bytes!`s test fonts that are not shipped), so this file brings its
//! own minimal platform: a `MinimalSoftwareWindow` plus a mock clock, mirroring
//! what `firmware/src/platform.rs` does on the ESP32.
//!
//! Everything runs in one `#[test]` because Slint's global platform can only be
//! installed once per process.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ui::slint::platform::software_renderer::MinimalSoftwareWindow;
use ui::slint::platform::{Platform, WindowAdapter};
use ui::slint::PhysicalSize;
use ui::{App, NoopStore, Settings, SettingsStore, Sound, SoundSink};

// Mock clock: `mock_elapsed_time` moves it forward, so timers and animations
// advance deterministically.
thread_local! {
    static MOCK_NOW_MS: Cell<u64> = const { Cell::new(0) };
}

fn mock_elapsed_time(ms: u64) {
    MOCK_NOW_MS.with(|now| now.set(now.get() + ms));
}

struct HeadlessPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for HeadlessPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, ui::slint::PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> core::time::Duration {
        core::time::Duration::from_millis(MOCK_NOW_MS.with(Cell::get))
    }

    fn debug_log(&self, _arguments: core::fmt::Arguments) {}
}

fn install_platform() {
    let window = MinimalSoftwareWindow::new(Default::default());
    window.set_size(PhysicalSize::new(480, 480));
    ui::slint::platform::set_platform(Box::new(HeadlessPlatform { window })).unwrap();
}

#[derive(Default, Clone)]
struct Recorder(Rc<RefCell<Vec<ui::Sound>>>);

impl SoundSink for Recorder {
    fn play(&mut self, sound: ui::Sound) {
        self.0.borrow_mut().push(sound);
    }
}

impl Recorder {
    fn take(&self) -> Vec<ui::Sound> {
        core::mem::take(&mut self.0.borrow_mut())
    }
}

/// Records every save, and hands back `stored` on load (if set).
#[derive(Default, Clone)]
struct RecorderStore {
    saved: Rc<RefCell<Vec<Settings>>>,
    stored: Rc<RefCell<Option<Settings>>>,
}

impl RecorderStore {
    /// A store that starts empty, so the UI keeps its defaults.
    fn empty() -> Self {
        Self::default()
    }

    /// A store that restores `settings` at startup.
    fn holding(settings: Settings) -> Self {
        Self {
            saved: Rc::default(),
            stored: Rc::new(RefCell::new(Some(settings))),
        }
    }

    fn take(&self) -> Vec<Settings> {
        core::mem::take(&mut self.saved.borrow_mut())
    }
}

impl SettingsStore for RecorderStore {
    fn load(&mut self) -> Option<Settings> {
        self.stored.borrow_mut().take()
    }

    fn save(&mut self, settings: Settings) {
        self.saved.borrow_mut().push(settings);
    }
}

/// Delivers pending property notifications and the 1 ms coalescing timer: the
/// first turn runs the `changed` handlers, the second one saves.
fn pump() {
    for _ in 0..2 {
        mock_elapsed_time(2);
        ui::slint::platform::update_timers_and_animations();
    }
}

/// Ticks Slint's timers and animations, one second per step.
fn run_for(seconds: u32) {
    for _ in 0..seconds {
        mock_elapsed_time(1000);
        ui::slint::platform::update_timers_and_animations();
    }
}

/// Starts a session and lets the 120 ms arming timer fire, which is what
/// actually begins the first inhale.
fn start_session(pattern: i32, minutes: i32, recorder: &Recorder) -> App {
    let app = App::new(recorder.clone(), NoopStore).unwrap();
    app.component().invoke_select_pattern(pattern);
    app.component().set_minutes(minutes);
    app.show().unwrap();
    app.component().invoke_start_session_now();
    mock_elapsed_time(200);
    app
}

#[test]
fn ports_follow_the_ui_state() {
    install_platform();
    sound_port_follows_the_breathing_state_machine();
    settings_port_saves_every_change();
    settings_port_restores_stored_values();
}

fn sound_port_follows_the_breathing_state_machine() {
    // 4-4-4-4 ("Box") has holds, and they must be audible.
    let recorder = Recorder::default();
    let app = start_session(2, 1, &recorder);
    assert_eq!(app.component().get_screen(), ui::Screen::Session);
    // Inhale 4 s, so the hold begins on the 4th one-second tick after arming.
    run_for(5);
    assert_eq!(recorder.take(), [Sound::BreathIn, Sound::Hold]);
}

fn settings_port_saves_every_change() {
    let store = RecorderStore::empty();
    let app = App::new(NoopSinkStub, store.clone()).unwrap();
    let component = app.component();

    // Defaults are the "Awake" rhythm at 10 minutes, so no save has happened
    // yet: there was nothing to restore.
    assert_eq!(component.get_minutes(), 10);
    assert_eq!(component.get_inhale(), 6);
    assert_eq!(component.get_hold_inhale(), 0);
    assert_eq!(component.get_exhale(), 2);
    assert_eq!(component.get_hold_exhale(), 0);
    assert!(store.take().is_empty());

    component.set_minutes(7);
    pump();
    component.set_scent_on(true);
    pump();
    // Picking a preset goes through the numbers, not an index. It rewrites all
    // four steps, which must still reach the store as a single save.
    component.invoke_select_pattern(1);
    pump();

    let saved = store.take();
    assert_eq!(saved.len(), 3, "{saved:?}");
    assert_eq!(saved[0].minutes, 7);
    assert_eq!(saved[0].inhale, 6, "other fields must be carried along");
    assert_eq!(saved[1].scent_on, true);
    assert_eq!(
        (saved[2].inhale, saved[2].hold_inhale, saved[2].exhale, saved[2].hold_exhale),
        (4, 7, 8, 0),
        "Deep Calm"
    );

    // Out-of-range indices are ignored rather than reading out of bounds.
    component.invoke_select_pattern(99);
    component.invoke_select_pattern(-1);
    pump();
    assert!(store.take().is_empty(), "no change, so no save");
}

fn settings_port_restores_stored_values() {
    // 4-7-8-0 matches "Deep Calm"; the 7 minutes differ from the default.
    let store = RecorderStore::holding(Settings {
        minutes: 7,
        inhale: 4,
        hold_inhale: 7,
        exhale: 8,
        hold_exhale: 0,
        scent_on: true,
    });
    let app = App::new(NoopSinkStub, store.clone()).unwrap();
    let component = app.component();

    assert_eq!(component.get_minutes(), 7);
    assert_eq!(component.get_inhale(), 4);
    assert_eq!(component.get_hold_inhale(), 7);
    assert_eq!(component.get_exhale(), 8);
    assert_eq!(component.get_scent_on(), true);
    // Restoring is coalesced into one save, so the store never sees a partially
    // applied mix while the fields land one by one.
    pump();
    let restored = store.take();
    assert_eq!(restored.len(), 1, "{restored:?}");
    assert_eq!(restored[0].minutes, 7);
    assert_eq!(restored[0].scent_on, true);

    // A rhythm that matches no preset is custom, and the session uses its
    // numbers: 4 s inhale then 7 s hold, so the 5th tick enters the hold.
    let recorder = Recorder::default();
    let app = App::new(recorder.clone(), NoopStore).unwrap();
    app.component().set_inhale(4);
    app.component().set_hold_inhale(7);
    app.component().set_exhale(8);
    app.component().set_hold_exhale(0);
    app.component().set_minutes(6);
    app.component().invoke_start_session_now();
    mock_elapsed_time(200);
    run_for(4);
    recorder.take();
    run_for(1);
    assert_eq!(recorder.take(), [Sound::Hold]);
}

/// The sound port is not what these checks are about.
struct NoopSinkStub;

impl SoundSink for NoopSinkStub {
    fn play(&mut self, _sound: ui::Sound) {}
}