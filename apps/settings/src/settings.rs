//! The Settings window: its state, what each action does, and how its
//! screens are stacked and animated. The screens are drawn in the modules
//! below, all to Denial's mobile materials (Islands,
//! denial/docs/MOBILE_APP_MATERIALS.md): the home screen lists the pages in
//! group islands; a page holds its settings in section islands; Wi-Fi has
//! its own screens, in `networks`.

mod android;
mod bound;
mod cellular;
mod call_volume;
mod developer;
mod devices;
mod display_controls;
mod dialogs;
mod cursor;
mod fingers;
mod environment;
mod home;
mod input;
mod idle_power;
mod lights;
mod monitor;
mod networks;
mod output_modes;
mod page;
mod panel;
mod parts;
mod permissions;
mod position;
mod phone;
mod plugins;
mod profile;
mod power;
mod shortcuts;
mod system;
mod wallpapers;
mod welcome;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures_lite::FutureExt;

use denial_look::mobile::{GROUP_HEIGHT, ScrollAway, bottom_inset, content_bottom};
use denial_look::{INSET, Look, SettingsWatch, Tween, decelerate, ease_out_cubic, motion, stagger};
use denial_look_gpui::input::{Submitted, TextInput};
use denial_look_gpui::{
    Icon, backing, chip, color, control_radius, glass_group, icon, say, shadow, solid,
};
use gpui::{
    AnyElement, App, AsyncApp, Context, Div, ElementId, Entity, FocusHandle, Focusable, Font, KeyboardInset,
    ScrollHandle, SharedString, Stateful, Subscription, Task, WeakEntity, Window, actions, div, prelude::*,
    px,
};

use crate::about;
use crate::apps;
use crate::battery;
use crate::bluetooth;
use crate::catalog::{self, Content, Page};
use crate::clock;
use crate::country;
use crate::denial;
use crate::fingerprint;
use crate::location;
use crate::mobile;
use crate::values::Values;
use crate::wallpaper;
use crate::wifi;

actions!(settings, [Back]);

/// Items past this many arrive together: further ones are off screen.
const STAGGERED: usize = 12;
/// Logical pixels needed for the existing rows and controls in two panes.
const SIDEBAR_WIDTH: f32 = 320.0;
const DETAIL_MIN_WIDTH: f32 = 480.0;
/// A page beside the list is no wider than this: a row's value stays near
/// its name.
const DETAIL_MAX_WIDTH: f32 = 960.0;

#[derive(Clone)]
enum Screen {
    /// The pages, in their groups; with text in the search field, what it
    /// finds.
    Home,
    Page(&'static Page),
    /// The Wi-Fi networks the phone remembers.
    SavedNetworks,
    /// One saved network, by `wifi::Saved::id`, opened from the saved
    /// networks or from the Wi-Fi page.
    Network {
        id: SharedString,
        from_saved: bool,
    },
    /// One paired Bluetooth device, by its BlueZ object.
    Device {
        path: SharedString,
    },
    /// The wallpapers to pick from.
    Wallpapers,
    /// The wallpapers to pick from for the monitor beside the phone.
    MonitorWallpapers,
    /// The fingers that unlock the phone.
    Fingers,
    /// The time zones to pick from.
    Zones,
    /// The countries the Wi-Fi rules can follow.
    Countries,
    /// One Android app, by its package.
    AndroidApp {
        package: SharedString,
    },
}

impl PartialEq for Screen {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Screen::Home, Screen::Home)
            | (Screen::SavedNetworks, Screen::SavedNetworks)
            | (Screen::Wallpapers, Screen::Wallpapers)
            | (Screen::MonitorWallpapers, Screen::MonitorWallpapers)
            | (Screen::Fingers, Screen::Fingers)
            | (Screen::Zones, Screen::Zones)
            | (Screen::Countries, Screen::Countries) => true,
            (Screen::Page(a), Screen::Page(b)) => std::ptr::eq(*a, *b),
            (Screen::Network { id: a, .. }, Screen::Network { id: b, .. }) => a == b,
            (Screen::Device { path: a }, Screen::Device { path: b }) => a == b,
            (Screen::AndroidApp { package: a }, Screen::AndroidApp { package: b }) => a == b,
            _ => false,
        }
    }
}

impl Screen {
    /// The name its scroll position is kept under.
    fn key(&self) -> &'static str {
        match self {
            Screen::Home => "home",
            Screen::Page(page) => page.id,
            Screen::SavedNetworks => "saved-networks",
            Screen::Network { .. } => "network",
            Screen::Device { .. } => "device",
            Screen::Wallpapers => "wallpapers",
            Screen::MonitorWallpapers => "monitor-wallpapers",
            Screen::Fingers => "fingers",
            Screen::Zones => "zones",
            Screen::Countries => "countries",
            Screen::AndroidApp { .. } => "android-app",
        }
    }

    /// Whether this is the page with id `id`.
    fn is_page(&self, id: &str) -> bool {
        matches!(self, Screen::Page(page) if page.id == id)
    }

    /// Wi-Fi shows networks in range here, so it keeps looking for them.
    fn scans(&self) -> bool {
        matches!(self, Screen::Page(page) if matches!(page.content, Content::Wifi))
            || matches!(self, Screen::SavedNetworks | Screen::Network { .. })
    }

    /// The Display page shows the panel's brightness and refresh rate.
    fn shows_panel(&self) -> bool {
        matches!(self, Screen::Page(page) if matches!(page.content, Content::Display))
    }

    fn shows_mobile(&self) -> bool {
        matches!(self, Screen::Page(page) if matches!(page.content, Content::Mobile))
    }

    /// Bluetooth shows devices nearby here, so it keeps looking for them.
    fn looks(&self) -> bool {
        matches!(self, Screen::Page(page) if matches!(page.content, Content::Bluetooth))
            || matches!(self, Screen::Device { .. })
    }
}

/// Which of the screens drawn at once: the one shown, or the one leaving
/// during a switch, which takes no input.
#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Shown,
    Leaving,
}

impl Layer {
    fn live(self) -> bool {
        self == Layer::Shown
    }
}

/// Where an island is drawn while its screen comes or goes: faded and moved
/// down by `offset` lp.
#[derive(Clone, Copy)]
struct Motion {
    opacity: f32,
    offset: f32,
}

/// How far the screen pushed aside moves left, in screen widths, as the
/// new one slides over it.
const PARALLAX: f32 = 0.3;

/// How the shown screen came in: at launch, its islands rising one after
/// another; opened from another screen, sliding in from the right over it
/// as it moves left and fades; or back from one, which slides away to the
/// right.
struct Arrival {
    clock: Tween,
    launch: bool,
    back: bool,
    leaving: Option<Screen>,
}

impl Arrival {
    /// Island `index`, counted from the top: only launch moves islands one
    /// by one; a push moves the whole screen.
    fn item(&self, index: usize, now: Instant) -> Motion {
        if !self.launch {
            return Motion {
                opacity: 1.0,
                offset: 0.0,
            };
        }
        let delay = stagger(
            motion::LAUNCH_DELAY,
            motion::LAUNCH_STEP,
            index.min(STAGGERED),
        );
        let t = ease_out_cubic(self.clock.segment(delay, motion::LAUNCH).progress(now));
        Motion {
            opacity: t,
            offset: motion::LAUNCH_RISE * (1.0 - t),
        }
    }

    /// The contents of the top group, which fade in at launch.
    fn group(&self, now: Instant) -> f32 {
        if self.launch {
            self.clock.segment(0, motion::GROUP_FADE).progress(now)
        } else {
            1.0
        }
    }

    /// How far the push has come, 0 to 1, while the leaving screen shows.
    fn gone(&self, now: Instant) -> Option<f32> {
        let push = self.clock.segment(0, motion::SLIDE);
        (self.leaving.is_some() && push.running(now)).then(|| decelerate(push.progress(now)))
    }

    /// Where a screen, and its top group's contents, are during a push:
    /// moved right by so many widths, and faded.
    fn shift(&self, layer: Layer, now: Instant) -> (f32, f32) {
        let Some(push) = self.gone(now) else {
            return (0.0, 1.0);
        };
        match (layer, self.back) {
            (Layer::Shown, false) => (1.0 - push, 1.0),
            (Layer::Leaving, false) => (-PARALLAX * push, 1.0 - push),
            (Layer::Shown, true) => (-PARALLAX * (1.0 - push), push),
            (Layer::Leaving, true) => (push, 1.0),
        }
    }

    /// Beside the list a page does not slide: what leaves fades out, 1 to
    /// 0, while it still shows.
    fn fading(&self, now: Instant) -> Option<f32> {
        let out = self.clock.segment(0, motion::OUT);
        (!self.launch && self.leaving.is_some() && out.running(now))
            .then(|| 1.0 - out.progress(now))
    }

    /// Island `index` of the page that arrives beside the list: once what
    /// left has faded, the islands rise in, the nearest first.
    fn rising(&self, index: usize, now: Instant) -> Motion {
        if self.launch {
            return self.item(index, now);
        }
        let delay = stagger(motion::OUT, motion::IN_STEP, index.min(STAGGERED));
        let t = decelerate(self.clock.segment(delay, motion::IN).progress(now));
        Motion {
            opacity: t,
            offset: motion::IN_RISE * (1.0 - t),
        }
    }

    fn running(&self, now: Instant) -> bool {
        let end = if self.launch {
            stagger(motion::LAUNCH_DELAY, motion::LAUNCH_STEP, STAGGERED) + motion::LAUNCH
        } else {
            // The slide of a narrow window, or the fade and rise of a wide.
            motion::SLIDE.max(stagger(motion::OUT, motion::IN_STEP, STAGGERED) + motion::IN)
        };
        self.clock.segment(end, 0).running(now)
    }
}

/// How much of the window the on-screen keyboard covers, in lp, as the shell
/// moves it from `from` to `to` over `motion`.
struct Keyboard {
    from: f32,
    to: f32,
    motion: Tween,
}

impl Keyboard {
    /// The height now, decelerating like the shell's keyboard.
    fn height(&self, now: Instant) -> f32 {
        denial_look::lerp(
            self.from,
            self.to,
            ease_out_cubic(self.motion.progress(now)),
        )
    }
}

/// What runs when one of a dialog's options is picked.
type OnPick = Rc<dyn Fn(&mut Settings, usize, &mut Window, &mut Context<Settings>)>;
/// What runs when a dialog is confirmed.
type OnConfirm = Rc<dyn Fn(&mut Settings, &mut Window, &mut Context<Settings>)>;

/// A dialog on the gesture zone, over the dimmed screen.
#[derive(Clone)]
enum Dialog {
    /// One of a few options, the picked one marked.
    OutputScale { name: String },
    OutputPosition { name: String },
    ProfilePhoto { image: std::sync::Arc<gpui::Image> },
    TextEdit {
        title: &'static str,
        detail: &'static str,
        valid: Rc<dyn Fn(&str) -> bool>,
        save: Rc<dyn Fn(&mut Settings, String, &mut Context<Settings>)>,
    },
    Pick {
        title: SharedString,
        options: Vec<SharedString>,
        picked: usize,
        pick: OnPick,
    },
    /// Something that cannot be undone.
    Confirm {
        title: SharedString,
        consequence: SharedString,
        action: &'static str,
        confirm: OnConfirm,
    },
    /// The password of a network, to join it; `error` says why it is asked
    /// again.
    Password {
        network: wifi::Network,
        error: Option<&'static str>,
    },
    /// A network that does not say its name; `error` says why it is asked
    /// again.
    Hidden { error: Option<&'static str> },
    /// The SIM's PIN; `error` says why it is asked again.
    Pin { error: Option<&'static str> },
    /// What Settings' Bluetooth agent asks while pairing with `name`.
    Pairing {
        name: SharedString,
        request: bluetooth::Request,
    },
    /// The phone's name, as nearby devices see it.
    Rename,
    /// Whether to keep a screen's new refresh rate or scale, `value`, or go
    /// back to `back`; Denial keeps the change under `token`.
    Keep {
        token: u64,
        value: SharedString,
        back: SharedString,
    },
    /// The account password and a new lock PIN, twice: the first PIN, or
    /// one forgotten. `fingers` when the fingerprints asked for it, which
    /// open once it is set. `error` says why it is asked again.
    LockPin {
        error: Option<&'static str>,
        fingers: bool,
    },
    /// The current lock PIN and a new one, twice.
    ChangePin { error: Option<&'static str> },
    /// The lock PIN, before the fingerprints open.
    CheckPin { error: Option<&'static str> },
    /// The account password, to remove the lock PIN.
    ClearPin { error: Option<&'static str> },
    /// A name for a fingerprint to add, or for `finger`, which can also be
    /// removed here.
    Finger { finger: Option<String> },
    /// The date, or the time of day, to set the clock to; `error` says why
    /// it is asked again.
    Clock {
        date: bool,
        error: Option<&'static str>,
    },
}

/// Wi-Fi as Settings knows it.
#[derive(Default)]
struct Wifi {
    state: wifi::State,
    link: Option<wifi::Link>,
    /// Read once at least; until then the page shows nothing.
    read: bool,
    /// The network being joined from here, by SSID, until it is or fails.
    joining: Option<Vec<u8>>,
    /// Why joining a network failed last, by SSID, until it is tried again.
    failed: HashMap<Vec<u8>, &'static str>,
}

/// Denial as Settings knows it: its settings document, the panel and the
/// volume.
struct Denial {
    link: denial::Link,
    /// None while deniald does not answer.
    document: Option<serde_json::Value>,
    /// Whether Denial's settings have been read, or found missing, once.
    read: bool,
    /// The panel and its brightness, read while the Display page shows.
    output: Option<denial::Output>,
    brightness: Option<f32>,
    /// The brightness the slider last asked for, and whether a request for
    /// it is on its way.
    wanted: Option<u8>,
    sending: bool,
    /// The media volume, read while the Sound page shows; as for the
    /// brightness, the level last asked for and whether it is on its way,
    /// and the last request's serial.
    volume: Option<f32>,
    volume_wanted: Option<u8>,
    volume_sending: bool,
    volume_serial: u32,
    audio_streams: Option<Vec<denial::AudioStream>>,
    audio_stream_revision: u64,
    audio_stream_writes: usize,
}

impl Denial {
    fn prefs(&self) -> Option<denial::Prefs> {
        self.document.as_ref().map(denial::Prefs::of)
    }
}

/// How the lock screen opens, read when the security page shows.
#[derive(Default)]
struct Lock {
    /// The lock PIN's length, 0 for the password; None until read, or when
    /// Denial's helper does not answer.
    pin_length: Option<u32>,
    /// A change is on its way to the helper.
    busy: bool,
}

/// The fingers as Settings knows them.
#[derive(Default)]
struct Fingers {
    /// fprintd's names for them; None without a reader, or until read.
    enrolled: Option<Vec<String>>,
    /// Whether the reader has been asked once.
    read: bool,
    /// The names given to fingerprints, by fprintd's slot.
    names: HashMap<String, String>,
    /// The finger being added, while it is.
    adding: Option<Adding>,
    /// A finger being removed.
    removing: Option<String>,
    /// Denial's fingerprint session, which adds and removes them, and the
    /// task following what it says; open after the PIN, while the
    /// fingerprints show.
    session: Option<(fingerprint::Session, Task<()>)>,
}

/// A finger being added and how far it is.
struct Adding {
    /// fprintd's slot, and the name it gets once added.
    finger: &'static str,
    name: String,
    progress: fingerprint::Progress,
    /// What the last touch was told, until the next one counts.
    again: Option<&'static str>,
    /// Where the sensor is, as `fingerprint::sensor` says.
    sensor: Option<[f32; 4]>,
}

/// The wallpapers as Settings knows them.
#[derive(Default)]
struct Wallpapers {
    /// What every screen shows, as the shell names it.
    current: Option<String>,
    choices: Vec<wallpaper::Choice>,
    /// Each picture's thumbnail, by its resource, once made.
    thumbs: HashMap<String, std::path::PathBuf>,
    /// Reading the pictures and making their thumbnails.
    reading: Option<Task<()>>,
    /// The tab the pictures come from.
    tab: wallpapers::Tab,
    lock_screen: bool,
    status: Option<Vec<String>>,
    status_task: Option<Task<()>>,
    /// What Wallhaven offers.
    online: wallpapers::Online,
    /// Where the shown wallpaper sits, as the sliders have it.
    placement: wallpaper::Placement,
    /// The screen they are for: the monitor by its connector, or None for
    /// the phone.
    output: Option<String>,
    /// What the monitor shows, for its page's row.
    monitor: Option<String>,
    /// The shown wallpaper whole, for the placement preview: its resource,
    /// the picture and its size.
    preview: Option<(String, std::path::PathBuf, (u32, u32))>,
}

/// What the phone is, read each time About opens.
#[derive(Default)]
struct About {
    facts: Option<about::Facts>,
    /// Denial's version, once deniald has said it.
    denial: Option<String>,
}

/// The storage, read when its page opens.
#[derive(Default)]
struct Storage {
    /// Bytes used and in all where the home folder is.
    used: Option<(u64, u64)>,
    /// Bytes in the home folder, once counted.
    home: Option<u64>,
    /// Counting them.
    counting: Option<Task<()>>,
}

/// What the Apps page shows, read when it opens.
#[derive(Default)]
struct Installed {
    apps: Vec<apps::App>,
    /// Droidloom starts with the session.
    droidloom: bool,
}

/// The battery as Settings knows it.
#[derive(Default)]
struct Power {
    /// None while UPower does not run.
    state: Option<battery::State>,
    changing_limits: HashSet<zbus::zvariant::OwnedObjectPath>,
    /// Batteries whose bypass charging is being changed, by supply name.
    changing_holds: HashSet<String>,
    /// Whether UPower has been read, or found missing, once.
    read: bool,
}

/// The mobile network as Settings knows it.
#[derive(Default)]
struct Mobile {
    state: mobile::State,
    /// None while dmail does not run.
    link: Option<mobile::Link>,
    /// Heard from dmail once at least; until then the page shows nothing.
    read: bool,
    /// The recovery step running, as its row says it meanwhile.
    busy: Option<&'static str>,
    /// Bytes received and sent on the data link, read while the page shows.
    traffic: Option<(u64, u64)>,
}

/// The GPS as Settings knows it.
#[derive(Default)]
struct Location {
    state: location::State,
    /// None while dmail does not run, or has no location.
    link: Option<location::Link>,
    /// Heard from dmail once at least; until then the page shows no GPS.
    read: bool,
    /// The user has the page locate the phone: Settings asks dmail for
    /// location until the switch goes off or the page closes.
    locating: bool,
    /// A Start or Stop on its way to dmail.
    asking: bool,
}

/// Bluetooth as Settings knows it.
#[derive(Default)]
struct Bluetooth {
    state: bluetooth::State,
    /// None while BlueZ does not run.
    link: Option<bluetooth::Link>,
    /// Heard from BlueZ once at least; until then the page shows nothing.
    read: bool,
    /// The device being paired, connected or disconnected from here, and
    /// what its row says meanwhile.
    busy: Option<(String, &'static str)>,
    /// Why the last action on a device failed, by its object, until the
    /// next one.
    failed: HashMap<String, &'static str>,
}

pub struct Settings {
    look: Look,
    input: input::Input,
    font: Font,
    focus_handle: FocusHandle,
    values: Values,
    search: Entity<TextInput>,
    /// The home screen's top group holds the search field.
    searching: bool,
    permissions: permissions::Permissions,
    plugins: plugins::Plugins,
    developer: developer::Developer,
    profile: profile::Profile,
    environment_scope: Option<String>,
    environment_editor: environment::Editor,
    /// Narrows the applications listed, names an override and gives it
    /// its value.
    environment_search: Entity<TextInput>,
    environment_name: Entity<TextInput>,
    environment_value: Entity<TextInput>,
    shortcuts: shortcuts::Shortcuts,
    /// The first-start tour, when this is Welcome and not Settings.
    welcome: Option<welcome::Welcome>,
    expanded_sections: HashSet<&'static str>,
    cursors: cursor::Cursors,
    /// Rows are being made for the panel inside an opened card: their
    /// corners follow the panel's, not an island's.
    nested: std::cell::Cell<bool>,
    /// When each element `appearing` builds was first built, by id: kept
    /// while every render builds it, so a fade plays once however the
    /// element is drawn. Ids a render left out are forgotten after it.
    appeared: std::cell::RefCell<Appeared>,
    /// The home screen's top group leaves as the pages scroll on, and comes
    /// back as they scroll back.
    top_away: ScrollAway,
    /// Each screen's scroll position, by `Screen::key`. A page opened afresh
    /// starts at its top; one gone back to is where it was left.
    scrolls: HashMap<&'static str, ScrollHandle>,
    dialog: Option<(Dialog, Tween)>,
    /// The password dialog's field, and whether it shows what is typed.
    password: Entity<TextInput>,
    revealed: bool,
    /// The hidden network dialog's name field.
    hidden_name: Entity<TextInput>,
    /// The SIM PIN dialog's field.
    pin: Entity<TextInput>,
    /// The lock PIN dialogs' fields: the PIN set now, a new one and the new
    /// one again.
    current_pin: Entity<TextInput>,
    new_pin: Entity<TextInput>,
    pin_again: Entity<TextInput>,
    /// The pairing dialog's field, for a PIN or passkey to type.
    code_field: Entity<TextInput>,
    /// The phone's Bluetooth name, while renaming.
    name_field: Entity<TextInput>,
    /// A fingerprint's name, while naming one.
    finger_field: Entity<TextInput>,
    /// The date or the time, while setting the clock.
    clock_field: Entity<TextInput>,
    output_field: Entity<TextInput>,
    /// What to look for on Wallhaven.
    wallhaven_field: Entity<TextInput>,
    /// What to look for among the countries.
    country_field: Entity<TextInput>,
    /// The slider under a finger or the mouse.
    /// The key of the slider the finger holds.
    sliding: Option<ElementId>,
    /// A short note above the gesture zone, until its task hides it.
    notice: Option<(SharedString, Tween, Task<()>)>,
    screen: Screen,
    arrival: Arrival,
    /// The on-screen keyboard, while a field has it.
    keyboard: Keyboard,
    wifi: Wifi,
    mobile: Mobile,
    location: Location,
    power: Power,
    /// The device's own lights, read once: they are part of it.
    lights: Vec<crate::lights::Light>,
    denial: Denial,
    display_controls: display_controls::DisplayControls,
    output_modes: output_modes::OutputModes,
    lock: Lock,
    fingers: Fingers,
    about: About,
    /// The clock as timedated says; None while it does not answer.
    clock: Option<clock::State>,
    /// The country the Wi-Fi rules follow; None while none is set.
    country: Option<&'static str>,
    storage: Storage,
    installed: Installed,
    wallpapers: Wallpapers,
    bluetooth: Bluetooth,
    android: android::Android,
    /// The monitor beside the phone, read every few seconds; None without
    /// one.
    monitor: Option<denial::Monitor>,
    _monitor: Task<()>,
    /// The densities Droidloom gives Android apps by screen, read when the
    /// Monitor or Android page opens.
    densities: crate::droidloom::Densities,
    /// Device pixels per pixel of the window, as the compositor scales it.
    scale: f32,
    /// The window is wide enough for the list beside a page.
    wide: bool,
    /// The window in device pixels, wide by high: the phone's screen.
    window_pixels: (u32, u32),
    _settings: Task<()>,
    _wifi: Task<()>,
    /// Joining a network, until it is joined or fails.
    _joining: Task<()>,
    _mobile: Task<()>,
    _location: Task<()>,
    _power: Task<()>,
    _display: Task<()>,
    /// Reading the panel again every second, while the Display page shows.
    reading_panel: Option<Task<()>>,
    /// Reading the volume again every second, while the Sound page shows.
    reading_volume: Option<Task<()>>,
    call_volume: call_volume::State,
    /// Reading the clock again every second, while Date and time shows.
    reading_clock: Option<Task<()>>,
    /// Reading the data link's traffic every two seconds, while Mobile
    /// network shows; its times move on meanwhile.
    reading_mobile: Option<Task<()>>,
    /// Unlocking the SIM, until dmail answers.
    _unlocking: Task<()>,
    _bluetooth: Task<()>,
    /// Pairing, connecting or disconnecting a device, until it is done.
    _device_action: Task<()>,
    /// Looks for Bluetooth devices while a Bluetooth screen is shown.
    looking: Option<Task<()>>,
    /// Asks for networks in range every few seconds, while a Wi-Fi screen
    /// shows them.
    scanning: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Settings {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Settings {
    /// Opens on the home screen, or on the page with id `page`.
    pub fn new(
        page: Option<String>,
        welcome: bool,
        bus: Option<zbus::Connection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let look = Look::load();
        let search = cx.new(|cx| TextInput::new("Search settings", &look, 1, cx));
        let password = cx.new(|cx| {
            let mut input = TextInput::new("Password", &look, 1, cx);
            input.set_secret(true, cx);
            input
        });
        let hidden_name = cx.new(|cx| TextInput::new("Network name", &look, 1, cx));
        let environment_search = cx.new(|cx| TextInput::new("Search applications", &look, 1, cx));
        let environment_name =
            cx.new(|cx| TextInput::new("e.g. MOZ_ENABLE_WAYLAND", &look, 1, cx));
        let environment_value = cx.new(|cx| TextInput::new("e.g. 1", &look, 1, cx));
        let code_field = cx.new(|cx| TextInput::new("Code", &look, 1, cx));
        let name_field = cx.new(|cx| TextInput::new("Phone name", &look, 1, cx));
        let finger_field = cx.new(|cx| TextInput::new("Fingerprint name", &look, 1, cx));
        let output_field = cx.new(|cx| TextInput::new("", &look, 1, cx));
        let clock_field = cx.new(|cx| TextInput::new("", &look, 1, cx));
        let wallhaven_field = cx.new(|cx| TextInput::new("Search Wallhaven", &look, 1, cx));
        let input = input::Input::new(&look, cx);
        let country_field = cx.new(|cx| TextInput::new("Search countries", &look, 1, cx));
        let secret = |hint: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut input = TextInput::new(hint, &look, 1, cx);
                input.set_secret(true, cx);
                input
            })
        };
        let pin = secret("PIN", cx);
        let current_pin = secret("Current PIN", cx);
        let new_pin = secret("New PIN", cx);
        let pin_again = secret("New PIN again", cx);
        let mut subscriptions = vec![
            cx.observe(&input.layouts, |_, _, cx| cx.notify()),
            cx.observe(&input.options, |_, _, cx| cx.notify()),
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&password, |_, _, cx| cx.notify()),
            cx.observe(&hidden_name, |_, _, cx| cx.notify()),
            cx.observe(&environment_search, |_, _, cx| cx.notify()),
            cx.observe(&environment_name, |this, _, cx| this.typed_environment(cx)),
            cx.observe(&environment_value, |this, _, cx| this.typed_environment(cx)),
            cx.subscribe_in(
                &environment_name,
                window,
                |this, _, _: &Submitted, window, cx| this.submit_environment_name(window, cx),
            ),
            cx.subscribe_in(
                &environment_value,
                window,
                |this, _, _: &Submitted, window, cx| this.save_environment_variable(window, cx),
            ),
            cx.observe(&pin, |_, _, cx| cx.notify()),
            cx.observe(&current_pin, |_, _, cx| cx.notify()),
            cx.observe(&new_pin, |_, _, cx| cx.notify()),
            cx.observe(&pin_again, |_, _, cx| cx.notify()),
            cx.observe(&code_field, |_, _, cx| cx.notify()),
            cx.observe(&name_field, |_, _, cx| cx.notify()),
            cx.observe(&finger_field, |_, _, cx| cx.notify()),
            cx.observe(&output_field, |_, _, cx| cx.notify()),
            cx.observe(&clock_field, |_, _, cx| cx.notify()),
            cx.observe(&wallhaven_field, |this, _, cx| this.typed_wallhaven(cx)),
            cx.observe(&country_field, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &wallhaven_field,
                window,
                |this, _, _: &Submitted, window, cx| this.submit_wallhaven(window, cx),
            ),
            cx.subscribe_in(&output_field, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(
                &clock_field,
                window,
                |this, _, _: &Submitted, window, cx| this.submit_dialog(window, cx),
            ),
            cx.subscribe_in(
                &finger_field,
                window,
                |this, _, _: &Submitted, window, cx| this.submit_dialog(window, cx),
            ),
            cx.subscribe_in(&code_field, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(&name_field, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(&pin, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(
                &current_pin,
                window,
                |this, _, _: &Submitted, window, cx| this.submit_dialog(window, cx),
            ),
            cx.subscribe_in(&new_pin, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(&pin_again, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(&password, window, |this, _, _: &Submitted, window, cx| {
                this.submit_dialog(window, cx)
            }),
            cx.subscribe_in(
                &hidden_name,
                window,
                |this, _, _: &Submitted, window, cx| this.submit_dialog(window, cx),
            ),
        ];
        // Denial stops lifting the window for the keyboard and reports it
        // instead; elsewhere the window stays as it is.
        if window.handle_keyboard_inset() {
            subscriptions.push(cx.observe_keyboard_inset(window, |this, window, cx| {
                this.keyboard_moved(window.keyboard_inset(), window, cx)
            }));
        }
        // Before the first screen is picked: `settings developer` opens its
        // page only where Denial has something for a developer.
        let developer = developer::Developer::at_start();
        let screen = page
            .as_deref()
            .and_then(catalog::page)
            .map_or(Screen::Home, Screen::Page);
        let font = gpui::font(look.font_family(&cx.text_system().all_font_names()));
        let arrival = Arrival {
            clock: look.tween(0),
            launch: true,
            back: false,
            leaving: None,
        };
        let keyboard = Keyboard {
            from: 0.0,
            to: 0.0,
            motion: look.tween(0),
        };
        let mut settings = Self {
            look,
            input,
            font,
            focus_handle: cx.focus_handle(),
            values: Values::load(),
            search,
            searching: false,
            permissions: permissions::Permissions::default(),
            plugins: plugins::Plugins::default(),
            developer,
            profile: profile::Profile::default(),
            environment_scope: None,
            environment_editor: environment::Editor::default(),
            environment_search,
            environment_name,
            environment_value,
            shortcuts: shortcuts::Shortcuts::default(),
            welcome: welcome.then(welcome::Welcome::default),
            expanded_sections: HashSet::new(),
            cursors: cursor::Cursors::default(),
            nested: std::cell::Cell::new(false),
            appeared: Default::default(),
            top_away: ScrollAway::default(),
            scrolls: HashMap::new(),
            dialog: None,
            password,
            revealed: false,
            hidden_name,
            pin,
            current_pin,
            new_pin,
            pin_again,
            code_field,
            name_field,
            finger_field,
            clock_field,
            output_field,
            wallhaven_field,
            country_field,
            sliding: None,
            notice: None,
            screen,
            arrival,
            keyboard,
            wifi: Wifi::default(),
            _settings: cx.spawn(async move |this, cx| Self::follow_settings(this, cx).await),
            _wifi: cx.spawn(async move |this, cx| Self::follow_wifi(this, cx).await),
            _joining: Task::ready(()),
            mobile: Mobile::default(),
            _mobile: cx.spawn(async move |this, cx| Self::follow_mobile(this, cx).await),
            location: Location::default(),
            _location: cx.spawn(async move |this, cx| Self::follow_location(this, cx).await),
            power: Power::default(),
            lights: {
                let lights = crate::lights::lights();
                catalog::set_lights(!lights.is_empty());
                lights
            },
            _power: cx.spawn(async move |this, cx| Self::follow_power(this, cx).await),
            denial: Denial {
                link: denial::Link::new(),
                document: None,
                read: false,
                output: None,
                brightness: None,
                wanted: None,
                sending: false,
                volume: None,
                volume_wanted: None,
                volume_sending: false,
                volume_serial: 0,
                audio_streams: None,
                audio_stream_revision: 0,
                audio_stream_writes: 0,
            },
            display_controls: display_controls::DisplayControls::default(),
            output_modes: output_modes::OutputModes::default(),
            lock: Lock::default(),
            fingers: Fingers::default(),
            // The home screen names the model under About.
            about: About {
                facts: Some(about::read()),
                denial: None,
            },
            clock: None,
            storage: Storage::default(),
            installed: Installed::default(),
            wallpapers: Wallpapers {
                current: wallpaper::current(None),
                ..Wallpapers::default()
            },
            _display: cx.spawn(async move |this, cx| Self::follow_display(this, cx).await),
            reading_panel: None,
            reading_volume: None,
            call_volume: call_volume::State::new(bus),
            reading_clock: None,
            country: country::read(),
            reading_mobile: None,
            _unlocking: Task::ready(()),
            bluetooth: Bluetooth::default(),
            android: android::Android::default(),
            monitor: None,
            densities: Default::default(),
            _monitor: cx.spawn_in(window, async move |this, cx| {
                Self::follow_monitor(this, cx).await
            }),
            scale: 1.0,
            wide: false,
            window_pixels: (0, 0),
            _bluetooth: cx.spawn_in(window, async move |this, cx| {
                Self::follow_bluetooth(this, cx).await
            }),
            _device_action: Task::ready(()),
            looking: None,
            scanning: None,
            _subscriptions: subscriptions,
        };
        if !welcome && !settings.screen.is_page("sound") {
            settings.read_call_volume(cx);
        }
        settings.follow_screen(cx);
        settings
    }

    /// Takes Denial's look afresh whenever its settings change.
    async fn follow_settings(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let mut watch = match SettingsWatch::new() {
            Ok(watch) => watch,
            Err(error) => return eprintln!("settings: following Denial's settings: {error}"),
        };
        loop {
            if let Err(error) = watch.changed().await {
                return eprintln!("settings: following Denial's settings: {error}");
            }
            let look = cx.background_spawn(async { Look::load() }).await;
            let taken = this.update(cx, |view, cx| view.take_look(look, cx));
            if taken.is_err() {
                return;
            }
        }
    }

    fn take_look(&mut self, look: Look, cx: &mut Context<Self>) {
        self.font = gpui::font(look.font_family(&cx.text_system().all_font_names()));
        for input in [
            &self.search,
            &self.password,
            &self.hidden_name,
            &self.environment_search,
            &self.environment_name,
            &self.environment_value,
            &self.pin,
            &self.code_field,
            &self.name_field,
            &self.finger_field,
            &self.clock_field,
            &self.output_field,
            &self.wallhaven_field,
            &self.country_field,
            &self.input.layouts,
            &self.input.options,
        ] {
            input.update(cx, |input, cx| input.restyle(&look, cx));
        }
        self.look = look;
        cx.notify();
    }

    /// Follows the Wi-Fi manager for as long as the window lives.
    async fn follow_wifi(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let (updates, received) = async_channel::unbounded();
        let _follow = cx.background_spawn(wifi::follow(updates));
        while let Ok(update) = received.recv().await {
            let taken = this.update(cx, |view, cx| {
                match update {
                    wifi::Update::Link(link) => view.wifi.link = link,
                    wifi::Update::State(state) => {
                        catalog::set_wifi(state.device.is_some());
                        view.wifi.state = state;
                        view.wifi.read = true;
                    }
                }
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    /// Follows dmail for as long as the window lives.
    async fn follow_mobile(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let (updates, received) = async_channel::unbounded();
        let _follow = cx.background_spawn(mobile::follow(updates));
        while let Ok(update) = received.recv().await {
            let taken = this.update(cx, |view, cx| {
                let (link, state) = update.unzip();
                let mobile = &mut view.mobile;
                mobile.state = state.unwrap_or_default();
                mobile.traffic = mobile::traffic(&mobile.state.interface);
                mobile.link = link;
                mobile.read = true;
                catalog::set_mobile(mobile.link.is_some());
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    async fn follow_display(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let Ok(link) = this.update(cx, |view, _| view.denial.link.clone()) else {
            return;
        };
        let (updates, received) = async_channel::unbounded();
        let _follow = cx.background_spawn(denial::follow(link, updates));
        while let Ok(document) = received.recv().await {
            let taken = this.update(cx, |view, cx| {
                view.denial.document = document;
                view.denial.read = true;
                if view.screen == Screen::Wallpapers && view.wallpapers.lock_screen {
                    view.read_lock_wallpaper(cx);
                }
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    async fn follow_power(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let (updates, received) = async_channel::unbounded();
        let _follow = cx.background_spawn(battery::follow(updates));
        while let Ok(state) = received.recv().await {
            let taken = this.update(cx, |view, cx| {
                view.power.state = state;
                view.power.read = true;
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    /// Follows BlueZ for as long as the window lives, and answers its
    /// pairing questions.
    async fn follow_bluetooth(this: WeakEntity<Self>, cx: &mut gpui::AsyncWindowContext) {
        let (updates, received) = async_channel::unbounded();
        let (requests, asked) = async_channel::unbounded();
        let _follow = cx.background_spawn(bluetooth::follow(updates, requests));
        loop {
            let next = async { received.recv().await.map(Ok) }
                .or(async { asked.recv().await.map(Err) })
                .await;
            let taken = match next {
                Ok(Ok(update)) => this.update(cx, |view, cx| {
                    let bt = &mut view.bluetooth;
                    let (link, state) = update.unzip();
                    bt.link = link;
                    bt.state = state.unwrap_or_default();
                    bt.read = true;
                    cx.notify();
                }),
                Ok(Err(request)) => this.update_in(cx, |view, window, cx| {
                    view.pairing_request(request, window, cx)
                }),
                Err(_) => return,
            };
            if taken.is_err() {
                return;
            }
        }
    }

    /// Shows the page with id `id` when asked from outside, by
    /// `settings <page>` while Settings runs.
    pub fn show_page(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match catalog::page(id) {
            Some(page) => self.open(Screen::Page(page), window, cx),
            None => eprintln!("settings: no page {id}"),
        }
    }

    fn open(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        self.open_at(screen, false, window, cx);
    }

    /// Switches to `screen`: the shown one leaves as the new one arrives, in
    /// reverse when going `back`.
    fn open_at(&mut self, screen: Screen, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        if screen == self.screen {
            return;
        }
        self.dialog = None;
        self.sliding = None;
        let leaving = std::mem::replace(&mut self.screen, screen);
        if !back {
            self.scrolls.insert(self.screen.key(), ScrollHandle::new());
        }
        self.top_away = ScrollAway::shown(self.scrolled(&self.screen).unwrap_or(0.0));
        self.arrival = Arrival {
            clock: self.look.tween(0),
            launch: false,
            back,
            leaving: Some(leaving),
        };
        self.follow_screen(cx);
        // A page closes the on-screen keyboard; search keeps its text.
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Scans for networks while a Wi-Fi screen is shown, and looks for
    /// Bluetooth devices while a Bluetooth one is; each stops after.
    fn follow_screen(&mut self, cx: &mut Context<Self>) {
        self.follow_scanning(cx);
        self.follow_looking(cx);
        self.follow_panel(cx);
        self.follow_display_controls(cx);
        self.follow_output_modes(cx);
        self.follow_permissions(cx);
        self.follow_locating(cx);
        self.follow_plugins(cx);
        self.follow_developer(cx);
        self.follow_profile(cx);
        self.follow_shortcuts(cx);
        self.follow_volume(cx);
        self.follow_input(cx);
        self.follow_clock(cx);
        self.follow_traffic(cx);
        self.follow_android(cx);
        if self.screen.is_page("storage") {
            self.read_storage(cx);
        }
        if self.screen.is_page("apps") || self.screen.is_page("permissions") || self.screen.is_page("location") || self.screen.is_page("environment") || self.screen.is_page("shortcuts") {
            self.read_apps(cx);
        }
        if self.screen.is_page("time") || self.screen == Screen::Countries {
            self.country = country::read();
        }
        if self.screen.is_page("security") {
            self.read_lock(cx);
        }
        if self.screen.is_page("about") {
            self.read_about(cx);
        }
        if self.screen.is_page("style") {
            self.wallpapers.current = wallpaper::current(None);
            self.read_cursors(cx);
        }
        if self.screen.is_page("monitor") {
            self.read_monitor_wallpaper();
            self.read_densities();
        }
        if self.screen.is_page("android") {
            self.read_densities();
        }
        self.follow_wallpaper_status(cx);
        match self.screen {
            Screen::Wallpapers => self.read_wallpapers(None, cx),
            Screen::MonitorWallpapers => {
                let output = self.monitor.as_ref().map(|monitor| monitor.name.clone());
                self.read_wallpapers(output, cx)
            }
            _ => {}
        }
        if self.screen == Screen::Fingers {
            self.read_fingers(cx);
        } else {
            self.fingers.adding = None;
            self.fingers.removing = None;
            self.fingers.session = None;
        }
    }

    /// The shade changes the brightness too, so the Display page reads the
    /// panel again every second.
    fn follow_panel(&mut self, cx: &mut Context<Self>) {
        if !self.screen.shows_panel() {
            self.reading_panel = None;
            return;
        }
        if self.reading_panel.is_some() {
            return;
        }
        self.reading_panel = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(link) = this.update(cx, |view, _| view.denial.link.clone()) else {
                    return;
                };
                let panel = Self::read_panel(link).await;
                if this
                    .update(cx, |view, cx| view.take_panel(panel, cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        }));
    }

    /// Until there is Wi-Fi to scan with, it looks again every second.
    fn follow_scanning(&mut self, cx: &mut Context<Self>) {
        if !self.screen.scans() {
            self.scanning = None;
            return;
        }
        if self.scanning.is_some() {
            return;
        }
        self.scanning = Some(cx.spawn(async move |this, cx| {
            loop {
                let asked = this.update(cx, |view, _| {
                    let wifi = &view.wifi;
                    let device = wifi.state.device.clone().filter(|_| wifi.state.enabled);
                    wifi.link.clone().zip(device)
                });
                let wait = match asked {
                    Ok(Some((link, device))) => {
                        link.scan(&device).await;
                        wifi::RESCAN
                    }
                    Ok(None) => Duration::from_secs(1),
                    Err(_) => return,
                };
                cx.background_executor().timer(wait).await;
            }
        }));
    }

    /// Discovery stops when the controller does, so it is asked again every
    /// few seconds; on leaving, it stops.
    fn follow_looking(&mut self, cx: &mut Context<Self>) {
        if !self.screen.looks() {
            if self.looking.take().is_some() {
                let bt = &self.bluetooth;
                if let Some((link, adapter)) = bt.link.clone().zip(bt.state.adapter.clone()) {
                    cx.background_spawn(async move { link.stop_looking(&adapter).await })
                        .detach();
                }
            }
            return;
        }
        if self.looking.is_some() {
            return;
        }
        self.looking = Some(cx.spawn(async move |this, cx| {
            loop {
                let asked = this.update(cx, |view, _| {
                    let bt = &view.bluetooth;
                    let adapter = bt.state.adapter.clone().filter(|adapter| adapter.powered);
                    bt.link.clone().zip(adapter)
                });
                let wait = match asked {
                    Ok(Some((link, adapter))) => {
                        if let Err(error) = link.look(&adapter).await {
                            eprintln!("settings: looking for Bluetooth devices: {error}");
                        }
                        bluetooth::REDISCOVER
                    }
                    Ok(None) => Duration::from_secs(1),
                    Err(_) => return,
                };
                cx.background_executor().timer(wait).await;
            }
        }));
    }

    /// The screen Back goes to from `screen`.
    fn parent(screen: &Screen) -> Screen {
        let wifi = catalog::page("wifi").map_or(Screen::Home, Screen::Page);
        match screen {
            Screen::Network {
                from_saved: true, ..
            } => Screen::SavedNetworks,
            Screen::Network { .. } | Screen::SavedNetworks => wifi,
            Screen::Device { .. } => catalog::page("bluetooth").map_or(Screen::Home, Screen::Page),
            Screen::Wallpapers => catalog::page("style").map_or(Screen::Home, Screen::Page),
            Screen::MonitorWallpapers => {
                catalog::page("monitor").map_or(Screen::Home, Screen::Page)
            }
            Screen::Fingers => catalog::page("security").map_or(Screen::Home, Screen::Page),
            Screen::Zones | Screen::Countries => {
                catalog::page("time").map_or(Screen::Home, Screen::Page)
            }
            Screen::AndroidApp { .. } => {
                catalog::page("android").map_or(Screen::Home, Screen::Page)
            }
            Screen::Home | Screen::Page(_) => Screen::Home,
        }
    }

    /// Denial's Back gesture, Escape and the top group's back button: undoes
    /// the innermost state. A dialog closes; then search ends and its field
    /// clears; then a screen goes back to the one it came from; at home, the
    /// window closes.
    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        if self.welcome.is_some() {
            self.welcome_back(cx);
        } else if self.dialog.is_some() {
            self.dismiss(window, cx);
        } else if (self.searching || (self.wide && !self.search.read(cx).text().is_empty()))
            && (self.screen == Screen::Home
                || self.search.read(cx).focus_handle(cx).is_focused(window))
        {
            self.end_search(window, cx);
        } else if self.screen == Screen::Home {
            window.remove_window();
        } else {
            let parent = Self::parent(&self.screen);
            self.open_at(parent, true, window, cx);
        }
    }

    fn start_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = true;
        self.top_away = ScrollAway::shown(self.scrolled(&Screen::Home).unwrap_or(0.0));
        window.focus(&self.search.focus_handle(cx), cx);
        cx.notify();
    }

    fn end_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searching = false;
        self.search.update(cx, |search, cx| search.clear(cx));
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The scroll position of `screen`, made when first asked for.
    fn scroll(&mut self, screen: &Screen) -> ScrollHandle {
        self.scrolls.entry(screen.key()).or_default().clone()
    }

    /// How far `screen` is scrolled, lp, once it has been drawn.
    fn scrolled(&self, screen: &Screen) -> Option<f32> {
        self.scrolls
            .get(screen.key())
            .map(|scroll| -f32::from(scroll.offset().y))
    }

    /// Moves the top group with the home screen's scroll. It stays while
    /// searching, and on pages.
    fn follow_scroll(&mut self) {
        match self.scrolled(&self.screen) {
            Some(offset) if self.screen == Screen::Home && !self.searching => {
                self.top_away.follow(offset, INSET + GROUP_HEIGHT);
            }
            offset => self.top_away = ScrollAway::shown(offset.unwrap_or(0.0)),
        }
    }

    fn show(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        self.dialog = Some((dialog, self.look.tween(0)));
        cx.notify();
    }

    /// Closes the dialog, and the keyboard with it. A password or PIN typed
    /// in it goes too.
    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Closing a pairing question answers it: no.
        if matches!(self.dialog, Some((Dialog::Pairing { .. }, _))) {
            return self.answer_pairing(None, window, cx);
        }
        // Closing the refresh-rate question goes back to the old rate.
        if let Some((Dialog::Keep { token, .. }, _)) = self.dialog {
            return self.answer_keep(token, false, window, cx);
        }
        if self.dialog.take().is_some() {
            self.clear_secrets(cx);
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    /// Shows `note` on a chip above the gesture zone for a moment.
    fn note(&mut self, note: impl Into<SharedString>, cx: &mut Context<Self>) {
        let hide = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(motion::NOTICE))
                .await;
            let _ = this.update(cx, |view, cx| {
                view.notice = None;
                cx.notify();
            });
        });
        self.notice = Some((say(note), self.look.tween(motion::CHIP_FADE), hide));
        cx.notify();
    }

    /// Follows the keyboard to its new height, over the shell's motion there.
    fn keyboard_moved(
        &mut self,
        inset: KeyboardInset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The user put the keyboard away, by dragging it down or with Back:
        // the field it typed into lets go of the focus, so a tap brings it back.
        if inset.height == px(0.) && self.keyboard.to > 0.0 {
            window.focus(&self.focus_handle, cx);
        }
        self.keyboard = Keyboard {
            from: self.keyboard.height(Instant::now()),
            to: f32::from(inset.height),
            motion: self.look.tween(inset.duration.as_millis() as u64),
        };
        cx.notify();
    }

    fn animating(&self, now: Instant) -> bool {
        let tweens = [
            self.dialog
                .as_ref()
                .map(|(_, shown)| shown.segment(0, motion::DIM)),
            self.notice.as_ref().map(|(_, fade, _)| *fade),
            Some(self.keyboard.motion),
        ];
        self.arrival.running(now) || tweens.iter().flatten().any(|tween| tween.running(now))
    }

    /// How far the screen is dimmed behind a dialog, 0 to 1.
    fn dim(&self, now: Instant) -> f32 {
        self.dialog.as_ref().map_or(0.0, |(_, shown)| {
            shown.segment(0, motion::DIM).progress(now)
        })
    }

    /// Island `index` of a screen: where it is while the window launches,
    /// and dimmed behind a dialog.
    fn item_motion(&self, layer: Layer, index: usize, now: Instant) -> Motion {
        let motion = self.arrival.item(index, now);
        let dim = if layer.live() {
            denial_look::lerp(1.0, motion::DIM_ALPHA, self.dim(now))
        } else {
            1.0
        };
        Motion {
            opacity: motion.opacity * dim,
            ..motion
        }
    }

    /// Padding under a screen's last island: clear of the gesture zone, or
    /// of the keyboard while it is up.
    fn list_bottom(&self, now: Instant) -> f32 {
        content_bottom(self.keyboard.height(now), 0.0, 0.0)
    }

    fn screen_parts(
        &self,
        screen: &Screen,
        layer: Layer,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        match screen {
            Screen::Home => self.home(None, layer, cx),
            Screen::Page(page) => match page.content {
                Content::Settings(sections) if page.id == "sound" => {
                    let mut parts = self.page(sections, layer, cx);
                    parts.push(
                        self.heading_row("Application audio", self.audio_actions(layer, cx)),
                    );
                    parts.push(self.application_audio(layer, cx));
                    parts
                }
                Content::Settings(sections) if page.id == "layout" => {
                    let mut parts = self.system_bar_displays(layer, cx);
                    parts.extend(self.page(sections, layer, cx));
                    parts
                }
                Content::Settings(sections) if page.id == "power" => {
                    let mut parts = self.charge_limit_parts(layer, cx);
                    parts.extend(self.page(sections, layer, cx));
                    parts
                }
                Content::Settings(sections) => self.page(sections, layer, cx),
                Content::Wifi => self.wifi_page(layer, cx),
                Content::Mobile => self.mobile_page(layer, cx),
                Content::Location => self.location_page(layer, cx),
                Content::Bluetooth => self.bluetooth_page(layer, cx),
                Content::Battery => self.battery_page(layer, cx),
                Content::Lights => self.lights_page(layer, cx),
                Content::Display => self.display_page(layer, cx),
                Content::OutputConfiguration => {
                    let mut parts = self.output_modes_page(width, layer, cx);
                    parts.extend(self.input_deck_page(layer, cx));
                    parts
                }
                Content::Input => self.input_page(layer, cx),
                Content::Permissions => self.permissions_page(layer, cx),
                Content::Plugins => self.plugins_page(layer, cx),
                Content::Developer => self.developer_page(layer, cx),
                Content::Profile => self.profile_page(layer, cx),
                Content::Environment => self.environment_page(width, layer, cx),
                Content::Shortcuts => self.shortcuts_page(layer, cx),
                Content::About => self.about_page(layer, cx),
                Content::AboutDenial => self.about_denial_page(width, layer, cx),
                Content::Storage => self.storage_page(),
                Content::Apps(sections) => self.apps_page(sections, layer, cx),
                Content::Android(sections) => self.android_page(sections, layer, cx),
                Content::Monitor(sections) => self.monitor_page(sections, layer, cx),
                Content::Hidden(_) | Content::Unavailable => Vec::new(),
            },
            Screen::SavedNetworks => self.saved_networks(layer, cx),
            Screen::Network { id, .. } => self.network(id, layer, cx),
            Screen::Device { path } => self.device_screen(path, layer, cx),
            Screen::Wallpapers | Screen::MonitorWallpapers => {
                self.wallpapers_screen(width, layer, cx)
            }
            Screen::Fingers => self.fingers_screen(layer, cx),
            Screen::Zones => self.zones_screen(layer, cx),
            Screen::Countries => self.countries_screen(layer, cx),
            Screen::AndroidApp { package } => self.android_app_screen(package, layer, cx),
        }
    }

    /// One screen: its islands, scrolling from the top group's upper edge to
    /// the window's lower one, under the group, the gesture zone and the
    /// keyboard, and its bottom zone; `width` wide, moved as a push has it.
    fn layer(
        &mut self,
        screen: &Screen,
        layer: Layer,
        width: f32,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = self.scroll(screen);
        let parts = self.screen_parts(screen, layer, width, cx);
        let this = cx.entity().downgrade();
        let content = div()
            .cull_offscreen()
            // A scroll shows the last frame moved, unless the top group
            // moves with it.
            .replay_scroll(move |_, _, cx| {
                this.update(cx, |this, _| {
                    let away = this.top_away.away();
                    this.follow_scroll();
                    this.top_away.away() != away
                })
                .unwrap_or(true)
            })
            .id(screen.key())
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(px(INSET))
            .pt(px(denial_look::mobile::content_top()))
            .pb(px(self.list_bottom(now)))
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(
                parts
                    .into_iter()
                    .enumerate()
                    .map(|(ix, part)| parts::place(part, self.item_motion(layer, ix, now))),
            );
        let (shift, opacity) = self.arrival.shift(layer, now);
        let mut layer_div = div()
            .id(match layer {
                Layer::Shown => "shown",
                Layer::Leaving => "leaving",
            })
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(shift * width))
            .w(px(width))
            .opacity(opacity)
            // Above the top group the gutter stays on the backing, which
            // Denial paints the status strip with.
            .child(
                div()
                    .absolute()
                    .top(px(INSET))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(content),
            );
        if layer.live() {
            layer_div = layer_div
                .child(self.bottom_zone(bottom_inset(self.keyboard.height(now)), now, cx))
                .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx)));
        }
        layer_div.into_any_element()
    }

    /// What sits on the gesture zone, or rides on the keyboard while it is
    /// up: a dialog, and a notice chip above it.
    fn bottom_zone(&self, inset: f32, now: Instant, cx: &mut Context<Self>) -> Div {
        let notice = self.notice.as_ref().map(|(note, fade, _)| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(gpui::relative(1.))
                .pb(px(INSET))
                .flex()
                .justify_center()
                .opacity(fade.progress(now))
                .child(chip(&self.look).child(note.clone()))
        });
        let apply = self.apply_button(cx);
        let dialog = self
            .dialog
            .as_ref()
            .map(|(dialog, shown)| self.dialog_island(dialog, *shown, now, cx));
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .px(px(INSET))
            .pb(px(inset))
            .children(notice)
            .children(apply)
            .children(dialog)
    }

    /// The button in the window's bottom corner while the shown page holds
    /// changes that are not carried out yet: it is there only then, so it
    /// is seen arriving. Faint while they cannot be carried out; away
    /// under a dialog.
    fn apply_button(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.dialog.is_some() {
            return None;
        }
        let (live, displays) = if self.screen.is_page("displays") {
            (self.output_changes()?, true)
        } else if self.screen.is_page("input") {
            (self.keyboard_changes(cx)?, false)
        } else {
            return None;
        };
        let button = div()
            .id("apply-changes")
            .flex_none()
            .h(px(denial_look::mobile::TOUCH))
            .pl(px(16.))
            .pr(px(20.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(control_radius(&self.look))
            .bg(solid(self.look.accent))
            .shadow(shadow(&self.look))
            .text_color(solid(self.look.on_accent))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(icon(
                Icon::Read,
                px(parts::ICON),
                solid(self.look.on_accent),
            ))
            .child(say("Apply"));
        let button = if live {
            self.tappable(button, cx, move |this, window, cx| {
                if displays {
                    this.apply_output_modes(cx);
                } else {
                    window.focus(&this.focus_handle, cx);
                    this.apply_keyboard(false, cx);
                }
            })
        } else {
            button.opacity(0.5)
        };
        Some(
            div()
                .absolute()
                .right(px(INSET + 10.))
                .bottom(gpui::relative(1.))
                .pb(px(10.))
                .child(self.appearing("apply-changes-shown", div().child(button))),
        )
    }

    /// The top group: the shown screen's contents, and the leaving one's
    /// during a push, each moving with its screen.
    fn top_group(&self, width: f32, now: Instant, cx: &mut Context<Self>) -> Div {
        let contents = |screen: &Screen, layer: Layer, cx: &mut Context<Self>| {
            let (shift, opacity) = self.arrival.shift(layer, now);
            div()
                .id(match layer {
                    Layer::Shown => "shown-top",
                    Layer::Leaving => "leaving-top",
                })
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(shift * width))
                .w_full()
                .opacity(opacity * self.arrival.group(now))
                .child(self.top_contents(screen, layer, true, cx))
        };
        let leaving = self
            .arrival
            .leaving
            .as_ref()
            .filter(|_| self.arrival.gone(now).is_some())
            .map(|screen| contents(screen, Layer::Leaving, cx));
        let shown = contents(&self.screen, Layer::Shown, cx);
        div()
            .absolute()
            .top(px(INSET - self.top_away.away()))
            .left(px(INSET))
            .right(px(INSET))
            .h(px(GROUP_HEIGHT))
            .child(
                glass_group(&self.look)
                    .size_full()
                    .relative()
                    .overflow_hidden()
                    .children(leaving)
                    .child(shown),
            )
    }

    /// What the top group holds on `screen`.
    fn top_contents(
        &self,
        screen: &Screen,
        layer: Layer,
        show_back: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let title: SharedString = match screen {
            Screen::Home => return self.home_top(layer, cx),
            Screen::Page(page) => page.title.into(),
            Screen::SavedNetworks => "Saved networks".into(),
            Screen::Network { id, .. } => self
                .wifi
                .state
                .saved(id)
                .map(|saved| saved.name.clone().into())
                .unwrap_or_default(),
            Screen::Device { path } => self
                .bluetooth
                .state
                .device(path)
                .map(|device| device.name.clone().into())
                .unwrap_or_default(),
            Screen::Wallpapers => "Wallpaper".into(),
            Screen::MonitorWallpapers => "Monitor wallpaper".into(),
            Screen::Fingers => "Fingerprint".into(),
            Screen::Zones => "Time zone".into(),
            Screen::Countries => "Country or region".into(),
            Screen::AndroidApp { package } => self
                .android_app(package)
                .map(|app| app.name.clone().into())
                .unwrap_or_default(),
        };
        self.top_row()
            .when(show_back, |row| row.child(self.back_button(layer, cx)))
            .child(self.title(title))
    }

    /// The sidebar selection stays on the parent page while a subpage is open.
    fn selected_page(&self) -> Option<&'static str> {
        let mut screen = self.screen.clone();
        loop {
            match screen {
                Screen::Home => return None,
                Screen::Page(page) => return Some(page.id),
                _ => screen = Self::parent(&screen),
            }
        }
    }

    fn wide_scroll(
        id: &'static str,
        scroll: &ScrollHandle,
        bottom: f32,
    ) -> Stateful<Div> {
        div()
            .cull_offscreen()
            .id(id)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(scroll)
            .flex()
            .flex_col()
            .gap(px(INSET))
            .px(px(INSET))
            .pt(px(INSET))
            .pb(px(bottom))
    }

    /// `pane`, a column that scrolls, and at its right a thin bar that says
    /// how far, shown while the pointer is over the pane. No bar when all
    /// of the column fits.
    fn marked(&self, group: &'static str, scroll: &ScrollHandle, pane: Stateful<Div>) -> Div {
        const WIDTH: f32 = 3.0;
        const EDGE: f32 = 2.0;
        const SHORTEST: f32 = 32.0;
        // As the pane was last laid out: a frame behind at most.
        let view = f32::from(scroll.bounds().size.height) - 2.0 * EDGE;
        let over = f32::from(scroll.max_offset().y);
        let mark = (view > SHORTEST && over > 0.0).then(|| {
            let length = (view * view / (view + over)).max(SHORTEST);
            let through = (-f32::from(scroll.offset().y) / over).clamp(0.0, 1.0);
            let shade = color(self.look.palette.foreground, 0.35);
            div()
                .absolute()
                .right(px(EDGE))
                .top(px(EDGE + through * (view - length)))
                .w(px(WIDTH))
                .h(px(length))
                .rounded_full()
                .group_hover(group, move |mark| mark.bg(shade))
        });
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .group(group)
            .child(pane)
            .children(mark)
    }

    fn wide_layout(&mut self, width: f32, now: Instant, cx: &mut Context<Self>) -> Div {
        // The page that leaves shows a moment longer, fading; then the one
        // that arrives rises in.
        let (screen, layer, fade) = match (self.arrival.fading(now), &self.arrival.leaving) {
            (Some(fade), Some(leaving)) => (leaving.clone(), Layer::Leaving, fade),
            _ => (self.screen.clone(), Layer::Shown, 1.0),
        };
        let home_scroll = self.scroll(&Screen::Home);
        let detail_scroll = self.scroll(&screen);
        let selected = self.selected_page();
        let bottom = INSET + self.keyboard.height(now);
        let dim = denial_look::lerp(1.0, motion::DIM_ALPHA, self.dim(now));
        let page_width = (width - SIDEBAR_WIDTH).min(DETAIL_MAX_WIDTH);

        let sidebar = div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .opacity(dim)
            .child(
                div()
                    .flex_none()
                    .h(px(GROUP_HEIGHT))
                    .px(px(INSET))
                    .child(self.home_top(Layer::Shown, cx)),
            )
            .child(self.marked(
                "list-pane",
                &home_scroll,
                Self::wide_scroll("sidebar-pages", &home_scroll, bottom).children(
                    self.home(selected, Layer::Shown, cx)
                        .into_iter()
                        .enumerate()
                        // Only launch moves the list's islands.
                        .map(|(ix, part)| parts::place(part, self.arrival.item(ix, now))),
                ),
            ));

        let detail = div()
            .id("settings-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .overflow_hidden();
        let detail = if screen == Screen::Home {
            detail.child(div().p(px(INSET)).child(self.notice_row("Select a setting")))
        } else {
            // A page is named by its row of the list, so it starts at the
            // top. What a page opens has no row: it keeps its name, and
            // the way back.
            let opened = !matches!(screen, Screen::Page(_));
            let parts = self.screen_parts(&screen, layer, page_width, cx);
            let header = opened.then(|| {
                div()
                    .flex_none()
                    .h(px(GROUP_HEIGHT))
                    .child(self.top_contents(&screen, layer, true, cx))
            });
            let arriving = layer.live();
            let motion = |index| {
                if arriving {
                    self.arrival.rising(index, now)
                } else {
                    Motion {
                        opacity: 1.0,
                        offset: 0.0,
                    }
                }
            };
            detail.child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .opacity(dim * fade)
                    .child(self.marked(
                        "page-pane",
                        &detail_scroll,
                        Self::wide_scroll(screen.key(), &detail_scroll, bottom)
                            .when(opened, |pane| pane.pt_0())
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(page_width))
                                    // In a pane wider than the page, the
                                    // page sits in its middle.
                                    .mx_auto()
                                    .flex()
                                    .flex_col()
                                    .gap(px(INSET))
                                    .children(
                                        header.map(|header| parts::place(header, motion(0))),
                                    )
                                    .children(
                                        parts.into_iter().enumerate().map(|(ix, part)| {
                                            parts::place(part, motion(ix + 1))
                                        }),
                                    ),
                            ),
                    )),
            )
        };
        div()
            .size_full()
            .flex()
            .child(sidebar)
            .child(
                detail
                    .child(self.bottom_zone(bottom, now, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))),
            )
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.scale = window.scale_factor();
        let viewport = window.viewport_size();
        self.window_pixels = (
            (f32::from(viewport.width) * self.scale).round() as u32,
            (f32::from(viewport.height) * self.scale).round() as u32,
        );
        self.follow_turn(cx);
        if self.animating(now) {
            window.request_animation_frame();
        }
        let root = self.render_root(window, now, cx);
        if self.appeared.borrow_mut().finish_render() {
            window.request_animation_frame();
        }
        root
    }
}

/// Ids `appearing` built in this render and in the ones before it.
#[derive(Default)]
struct Appeared {
    since: HashMap<gpui::ElementId, Instant>,
    built: HashSet<gpui::ElementId>,
    fading: bool,
}

impl Appeared {
    /// How long ago `id` was first built, counting this render.
    fn age(&mut self, id: gpui::ElementId, now: Instant) -> Duration {
        let since = *self.since.entry(id.clone()).or_insert(now);
        self.built.insert(id);
        now.saturating_duration_since(since)
    }

    /// Forgets the ids this render did not build; true while one fades.
    fn finish_render(&mut self) -> bool {
        let built = std::mem::take(&mut self.built);
        self.since.retain(|id, _| built.contains(id));
        std::mem::take(&mut self.fading)
    }
}

impl Settings {
    fn render_root(
        &mut self,
        window: &mut Window,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width);
        self.wide = width >= SIDEBAR_WIDTH + DETAIL_MIN_WIDTH;
        let root = div()
            .size_full()
            .relative()
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .bg(backing(&self.look))
            .font(self.font.clone())
            .text_color(solid(self.look.palette.foreground));
        if self.welcome.is_some() {
            return root.child(self.welcome_layout(width, cx));
        }
        if self.wide {
            return root
                .child(self.wide_layout(width, now, cx))
                .children(self.sensor_ring(viewport));
        }
        self.follow_scroll();
        let leaving = self
            .arrival
            .leaving
            .clone()
            .filter(|_| self.arrival.gone(now).is_some())
            .map(|screen| self.layer(&screen, Layer::Leaving, width, now, cx));
        let screen = self.screen.clone();
        let shown = self.layer(&screen, Layer::Shown, width, now, cx);
        root
            .children(leaving)
            .child(shown)
            .child(self.top_group(width, now, cx))
            .children(self.sensor_ring(viewport))
    }
}
