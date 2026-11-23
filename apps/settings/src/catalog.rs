//! What Settings lists: its pages, in groups on the home screen, and the
//! settings on each page, in sections. Wi-Fi, the mobile network,
//! Bluetooth, the display and the battery are real and have screens of
//! their own. A setting on a page says where its value lives (`Source`):
//! in Denial's settings document, in Denial's volume, worked out by
//! Settings itself, or made up for now, kept by the app (`values`) and
//! reaching nothing on the phone.

use denial_look_gpui::Icon;
use std::sync::atomic::{AtomicBool, Ordering};

/// Pages of one kind, one island on the home screen.
pub struct Group {
    pub title: &'static str,
    pub pages: &'static [Page],
}

pub struct Page {
    /// For `settings <page>`, and the start of its items' keys.
    pub id: &'static str,
    pub title: &'static str,
    pub icon: Icon,
    /// What the page holds, under its title on the home screen.
    pub summary: &'static str,
    pub content: Content,
}

pub enum Content {
    /// Settings in sections, kept by the app.
    Settings(&'static [Section]),
    /// The phone's Wi-Fi, through its manager: screens of their own.
    Wifi,
    /// The modem, through dmail.
    Mobile,
    /// The GPS through dmail, and the apps' answers to Denial's location
    /// question.
    Location,
    /// Bluetooth, through BlueZ.
    Bluetooth,
    /// The battery, through UPower.
    Battery,
    /// The display, through deniald.
    Display,
    OutputConfiguration,
    Input,
    Permissions,
    Profile,
    Environment,
    Shortcuts,
    /// The plugins the desktop is built from, through Denial's plugin
    /// manager. Listed only where it is installed.
    Plugins,
    /// The Flutter shell run live from its source, and the ways back to
    /// one that works. Listed only where Denial has something of it.
    Developer,
    /// What the phone is, from the kernel, BlueZ, dmail and deniald.
    About,
    AboutDenial,
    /// The storage: used and free, and what uses it.
    Storage,
    /// Settings in sections, then the apps installed.
    Apps(&'static [Section]),
    /// Android through Droidloom: the switch in sections, then how it runs
    /// and its apps.
    Android(&'static [Section]),
    /// The monitor beside the phone: its scale, then settings in sections.
    /// Listed only while one is plugged in.
    Monitor(&'static [Section]),
    /// Written, but shown nowhere until the phone has it.
    Hidden(#[expect(dead_code, reason = "shown once the phone has it")] &'static [Section]),
    /// Listed, but not on the phone yet: its row shows faint and opens
    /// nothing.
    Unavailable,
}

/// Settings that belong together: one island, under a chip when titled.
pub struct Section {
    pub title: Option<&'static str>,
    pub disclosure: Option<&'static str>,
    pub items: &'static [Item],
    /// Not on the phone yet: the island says this instead of its items,
    /// which stay here for when it is.
    pub pending: Option<&'static str>,
}

pub struct Item {
    /// Unique in the app: the page's id, a dot, the setting's name.
    pub key: &'static str,
    pub title: &'static str,
    pub detail: Option<&'static str>,
    pub icon: Option<Icon>,
    pub kind: Kind,
    pub source: Source,
    pub slider_unit: Option<SliderUnit>,
    /// Shown only while another row, by its key, has one of these options
    /// picked.
    pub only: Option<(&'static str, &'static [usize])>,
}

/// Where a setting's value lives.
#[derive(Clone, Copy)]
pub enum Source {
    /// Made up: kept by Settings, reaching nothing.
    Made,
    /// Not on the phone yet: the row shows faint on its kind's start and
    /// cannot be changed.
    Fixed,
    /// A key in Denial's settings document, by its path
    /// (`notifications.doNotDisturb`): a toggle's on or off.
    Denial(&'static str),
    /// A choice in Denial's document: the words its options are kept as.
    Words(&'static str, &'static [&'static str]),
    /// A number in Denial's document, from the first to the second, on a
    /// slider.
    Range(&'static str, f32, f32),
    /// The media volume, through Denial.
    Volume,
    /// Faint, showing the media volume: the phone has one volume for now.
    SameAsVolume,
    /// Worked out by Settings by the item's key: the screen lock, the
    /// lock's delay, the accent, the wallpaper.
    Own,
}

impl Source {
    /// Whether the row can be changed.
    pub fn changes(self) -> bool {
        !matches!(self, Source::Fixed | Source::SameAsVolume)
    }
}

#[derive(Clone, Copy)]
pub enum SliderUnit {
    Percent,
    Pixels,
    Degrees,
    Decimal,
    Multiplier,
    Count,
}

pub enum Kind {
    /// On or off, and which it starts as.
    Toggle(bool),
    /// A level from 0 to 1, and where it starts.
    Slider(f32),
    /// A few short options side by side, and the one it starts on.
    Segmented(&'static [&'static str], usize),
    /// Options picked in a dialog, and the one it starts on.
    Choice(&'static [&'static str], usize),
    /// A value to read, not to change.
    Info(&'static str),
    /// A button; the chip it shows says what it did.
    Action(&'static str),
    /// A row that opens a screen of its own, its value at its end.
    Opens,
}

const fn item(key: &'static str, title: &'static str, kind: Kind) -> Item {
    Item {
        key,
        title,
        detail: None,
        icon: None,
        kind,
        source: Source::Made,
        slider_unit: None,
        only: None,
    }
}

const fn measured(
    key: &'static str,
    title: &'static str,
    path: &'static str,
    initial: f32,
    min: f32,
    max: f32,
    unit: SliderUnit,
) -> Item {
    let mut item = item(key, title, Kind::Slider((initial - min) / (max - min)))
        .from(Source::Range(path, min, max));
    item.slider_unit = Some(unit);
    item
}

impl Item {
    const fn detail(mut self, detail: &'static str) -> Self {
        self.detail = Some(detail);
        self
    }

    const fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    const fn from(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    const fn fixed(self) -> Self {
        self.from(Source::Fixed)
    }

    const fn denial(self, path: &'static str) -> Self {
        self.from(Source::Denial(path))
    }

    const fn only(mut self, key: &'static str, options: &'static [usize]) -> Self {
        self.only = Some((key, options));
        self
    }
}

const fn section(title: &'static str, items: &'static [Item]) -> Section {
    Section {
        title: Some(title),
        items,
        pending: None,
        disclosure: None,
    }
}

/// A section with no chip over it: a page's first, main switch.
const fn lead(items: &'static [Item]) -> Section {
    Section {
        title: None,
        items,
        pending: None,
        disclosure: None,
    }
}

impl Section {
    const fn under(mut self, disclosure: &'static str) -> Self {
        self.disclosure = Some(disclosure);
        self
    }

    const fn pending(mut self, said: &'static str) -> Self {
        self.pending = Some(said);
        self
    }
}

/// The switch on the fingerprint screen, over the fingers.
pub static FINGERPRINT_UNLOCK: Item = item(
    "fingerprint.unlock",
    "Unlock with a fingerprint",
    Kind::Toggle(true),
)
.icon(Icon::Fingerprint)
.denial("lockScreen.fingerprintUnlock");

/// The accent colours to pick besides the wallpaper's, as Denial keeps
/// them: ARGB.
pub const ACCENTS: [(&str, u32); 6] = [
    ("Lavender", 0xFFD0BCFF),
    ("Blue", 0xFF8AB4F8),
    ("Teal", 0xFF5EC4C0),
    ("Green", 0xFF8FD9A8),
    ("Amber", 0xFFF2B35E),
    ("Rose", 0xFFF28AA8),
];

/// How long the session stays open once the screen goes dark, in seconds;
/// "Never" follows them in the choice.
pub const LOCK_DELAYS: [(&str, u32); 6] = [
    ("Instantly", 0),
    ("5 seconds", 5),
    ("30 seconds", 30),
    ("1 minute", 60),
    ("5 minutes", 5 * 60),
    ("10 minutes", 10 * 60),
];

const ANCHOR_LABELS: &[&str] = &[
    "Top left", "Top centre", "Top right", "Centre left", "Centre", "Centre right",
    "Bottom left", "Bottom centre", "Bottom right",
];
const ANCHOR_VALUES: &[&str] = &[
    "topLeft", "topCenter", "topRight", "centerLeft", "center", "centerRight",
    "bottomLeft", "bottomCenter", "bottomRight",
];
const EDGE_ANCHORS: &[usize] = &[0, 1, 2, 3, 5, 6, 7, 8];

pub static GROUPS: &[Group] = &[
    Group {
        title: "Connections",
        pages: &[
            Page {
                id: "wifi",
                title: "Wi-Fi",
                icon: Icon::Wifi,
                summary: "Networks, scanning, saved passwords",
                content: Content::Wifi,
            },
            Page {
                id: "mobile",
                title: "Mobile network",
                icon: Icon::Network,
                summary: "Data, SIM, network",
                content: Content::Mobile,
            },
            Page {
                id: "bluetooth",
                title: "Bluetooth",
                icon: Icon::Bluetooth,
                summary: "Paired devices, visibility",
                content: Content::Bluetooth,
            },
            Page {
                id: "hotspot",
                title: "Hotspot",
                icon: Icon::Hotspot,
                summary: "Share this phone's connection",
                content: Content::Unavailable,
            },
        ],
    },
    Group {
        title: "Device",
        pages: &[
            Page {
                id: "display",
                title: "Display",
                icon: Icon::Display,
                summary: "Brightness, appearance, timeout",
                content: Content::Display,
            },
            Page {
                id: "displays",
                title: "Display configuration",
                icon: Icon::FitScreen,
                summary: "Resolution, refresh rate, scale, rotation",
                content: Content::OutputConfiguration,
            },
            Page {
                id: "shortcuts",
                title: "Shortcuts",
                icon: Icon::Keyboard,
                summary: "Keyboard bindings and actions",
                content: Content::Shortcuts,
            },
            Page {
                id: "input",
                title: "Physical input",
                icon: Icon::Mouse,
                summary: "Keyboard layouts, mouse, touchpad",
                content: Content::Input,
            },
            Page {
                id: "layout",
                title: "Desktop layout",
                icon: Icon::Layout,
                summary: "Windows, workspaces, system bar",
                content: Content::Settings(&[
                    section("Windows", &[
                        item("layout.windows", "Window layout", Kind::Segmented(&["Stacking", "Dwindle", "Scrolling", "Full screen"], 0))
                            .from(Source::Words("layout.windowLayout", &["stacking", "dwindle", "scrolling", "screens"])),
                        item("layout.minimized", "Minimized windows", Kind::Segmented(&["On desktop", "Off-screen"], 1))
                            .from(Source::Words("layout.minimizedWindowPlacement", &["desktop", "offscreen"])),
                        measured(
                            "layout.maximizePadding", "Maximized window padding", "layout.maximizePadding",
                            8.0, 0.0, 64.0, SliderUnit::Pixels,
                        ),
                    ]),
                    section("Scrolling layout", &[
                        item("layout.preserveSwap", "Preserve sizes when swapping", Kind::Toggle(true))
                            .denial("layout.scrollingLayoutPreserveSwapSizes")
                            .only("layout.windows", &[2]),
                        measured(
                            "layout.wheelSpeed", "Scroll wheel speed", "layout.scrollingLayoutWheelSpeed",
                            1.0, 0.25, 4.0, SliderUnit::Percent,
                        ).only("layout.windows", &[2]),
                        item("layout.wheelDirection", "Scroll wheel up moves", Kind::Segmented(&["Left", "Right"], 0))
                            .from(Source::Words("layout.scrollingLayoutWheelUpDirection", &["left", "right"]))
                            .only("layout.windows", &[2]),
                    ]),
                    section("Workspaces", &[
                        item("layout.workspaces", "Enable workspaces", Kind::Toggle(true))
                            .denial("layout.workspacesEnabled")
                            .only("layout.windows", &[0, 1, 2]),
                        measured(
                            "layout.count", "Workspace count", "layout.workspaceCount",
                            4.0, 2.0, 9.0, SliderUnit::Count,
                        ).only("layout.workspaces", &[1]),
                        item("layout.orientation", "Workspace switching direction", Kind::Segmented(&["Horizontal", "Vertical"], 0))
                            .from(Source::Words("layout.workspaceSwitchingOrientation", &["horizontal", "vertical"]))
                            .only("layout.workspaces", &[1]),
                    ]),
                    section("System bar", &[
                        item("layout.barEdge", "System bar edge", Kind::Choice(&["Top", "Bottom", "Left", "Right"], 0))
                            .from(Source::Words("layout.systemBarSide", &["top", "bottom", "left", "right"])),
                        measured(
                            "layout.barThickness", "System bar thickness", "layout.systemBarThickness",
                            33.0, 24.0, 112.0, SliderUnit::Pixels,
                        ),
                    ]),
                    section("Clipboard tray", &[
                        item("layout.trayEdge", "Clipboard tray edge", Kind::Choice(&["Top", "Bottom", "Left", "Right"], 1))
                            .from(Source::Words("layout.clipboardTrayEdge", &["top", "bottom", "left", "right"])),
                        measured(
                            "layout.traySize", "Clipboard tray size", "layout.clipboardTrayExtent",
                            160.0, 100.0, 300.0, SliderUnit::Pixels,
                        ),
                    ]),
                    lead(&[item("layout.reset", "Reset desktop layout", Kind::Action("")).from(Source::Own)]),
                ]),
            },
            Page {
                id: "overlays",
                title: "Desktop overlays",
                icon: Icon::Copy,
                summary: "Launcher, dashboard, notifications, HUD",
                content: Content::Settings(&[
                    section("Launcher", &[
                        item("overlays.launcherAnchor", "Anchor", Kind::Choice(ANCHOR_LABELS, 4))
                            .from(Source::Words("overlays.launcher.anchor", ANCHOR_VALUES)),
                        measured("overlays.launcherWidth", "Width", "overlays.launcher.width", 730.0, 420.0, 1400.0, SliderUnit::Pixels),
                        measured("overlays.launcherHeight", "Height", "overlays.launcher.height", 200.0, 200.0, 1200.0, SliderUnit::Pixels),
                        measured("overlays.launcherMargin", "Edge distance", "overlays.launcher.margin", 8.0, 0.0, 96.0, SliderUnit::Pixels)
                            .only("overlays.launcherAnchor", EDGE_ANCHORS),
                        item("overlays.launcherHover", "Hover trigger", Kind::Toggle(true))
                            .denial("overlays.launcher.hoverTriggerEnabled")
                            .only("overlays.launcherAnchor", EDGE_ANCHORS),
                    ]).under("Launcher"),
                    section("Dashboard", &[
                        item("overlays.dashboardAnchor", "Anchor", Kind::Choice(ANCHOR_LABELS, 6))
                            .from(Source::Words("overlays.dashboard.anchor", ANCHOR_VALUES)),
                        measured("overlays.dashboardWidth", "Width", "overlays.dashboard.width", 420.0, 320.0, 1400.0, SliderUnit::Pixels),
                        measured("overlays.dashboardHeight", "Height", "overlays.dashboard.height", 740.0, 360.0, 1200.0, SliderUnit::Pixels),
                        measured("overlays.dashboardMargin", "Edge distance", "overlays.dashboard.margin", 8.0, 0.0, 96.0, SliderUnit::Pixels)
                            .only("overlays.dashboardAnchor", EDGE_ANCHORS),
                        item("overlays.dashboardHover", "Hover trigger", Kind::Toggle(true))
                            .denial("overlays.dashboard.hoverTriggerEnabled")
                            .only("overlays.dashboardAnchor", EDGE_ANCHORS),
                    ]).under("Dashboard"),
                    section("Notifications", &[
                        item("overlays.notificationsAnchor", "Anchor", Kind::Choice(ANCHOR_LABELS, 8))
                            .from(Source::Words("overlays.notifications.anchor", ANCHOR_VALUES)),
                        measured("overlays.notificationsWidth", "Width", "overlays.notifications.width", 410.0, 280.0, 1400.0, SliderUnit::Pixels),
                        measured("overlays.notificationsMargin", "Edge distance", "overlays.notifications.margin", 10.0, 0.0, 96.0, SliderUnit::Pixels)
                            .only("overlays.notificationsAnchor", EDGE_ANCHORS),
                    ]).under("Notifications"),
                    section("System HUD", &[
                        item("overlays.systemHudAnchor", "Anchor", Kind::Choice(ANCHOR_LABELS, 7))
                            .from(Source::Words("overlays.systemHud.anchor", ANCHOR_VALUES)),
                        measured("overlays.systemHudWidth", "Width", "overlays.systemHud.width", 350.0, 220.0, 1400.0, SliderUnit::Pixels),
                        measured("overlays.systemHudMargin", "Edge distance", "overlays.systemHud.margin", 10.0, 0.0, 96.0, SliderUnit::Pixels)
                            .only("overlays.systemHudAnchor", EDGE_ANCHORS),
                    ]).under("System HUD"),
                    lead(&[item("overlays.reset", "Reset overlay placement", Kind::Action("")).from(Source::Own)]),
                ]),
            },
            Page {
                id: "monitor",
                title: "Monitor",
                icon: Icon::Computer,
                summary: "Scale, windows, wallpaper",
                content: Content::Monitor(&[section(
                    "Desktop",
                    &[
                        item(
                            "monitor.layout",
                            "Windows",
                            Kind::Segmented(&["Stacking", "Tiling", "Scrolling", "Full screen"], 0),
                        )
                        .from(Source::Words(
                            "layout.windowLayout",
                            &["stacking", "dwindle", "scrolling", "screens"],
                        )),
                        item("monitor.corners", "Roundness", Kind::Slider(0.25)).from(
                            Source::Range("appearance.monitorCornerRadiusScale", 0.0, 2.0),
                        ),
                        item("monitor.wallpaper", "Wallpaper", Kind::Opens)
                            .icon(Icon::Photos)
                            .from(Source::Own),
                    ],
                )]),
            },
            Page {
                id: "sound",
                title: "Sound and vibration",
                icon: Icon::Speaker,
                summary: "Volume, ringtone, vibration",
                content: Content::Settings(&[
                    section(
                        "Volume",
                        &[
                            item("sound.media", "Media", Kind::Slider(0.5)).from(Source::Volume),
                            item("sound.ring", "Ring", Kind::Slider(0.7))
                                .from(Source::SameAsVolume),
                            item("sound.alarm", "Alarms", Kind::Slider(0.8))
                                .from(Source::SameAsVolume),
                        ],
                    ),
                    section(
                        "Calls",
                        &[
                            item(
                                "sound.ringtone",
                                "Ringtone",
                                Kind::Choice(&["Denial", "Glass", "Pulse", "Morning", "Silent"], 0),
                            )
                            .fixed(),
                            item("sound.vibrate", "Vibrate for calls", Kind::Toggle(true))
                                .icon(Icon::Vibrate)
                                .fixed(),
                        ],
                    ),
                    section(
                        "Feedback",
                        &[
                            item("sound.touch", "Touch sounds", Kind::Toggle(false)).fixed(),
                            item("sound.keys", "Keyboard vibration", Kind::Toggle(true))
                                .denial("sound.keyboardVibration"),
                        ],
                    ),
                ]),
            },
            Page {
                id: "notifications",
                title: "Notifications",
                icon: Icon::Bell,
                summary: "Do not disturb, lock screen",
                content: Content::Settings(&[
                    lead(&[
                        item("notifications.dnd", "Do not disturb", Kind::Toggle(false))
                            .detail("No popups, sounds or vibration")
                            .icon(Icon::Moon)
                            .denial("notifications.doNotDisturb"),
                    ]),
                    section(
                        "Lock screen",
                        &[
                            item(
                                "notifications.lock",
                                "On the lock screen",
                                Kind::Choice(&["Show all", "Show none"], 0),
                            )
                            .from(Source::Words(
                                "notifications.lockPreview",
                                &["full", "hidden"],
                            )),
                            item("notifications.wake", "Wake the screen", Kind::Toggle(true))
                                .detail("When a notification arrives")
                                .denial("notifications.wakeScreen"),
                        ],
                    ),
                    section(
                        "Recently sent",
                        &[
                            item(
                                "notifications.messages",
                                "Messages",
                                Kind::Info("2 min ago"),
                            )
                            .icon(Icon::Message),
                            item("notifications.calls", "Calls", Kind::Info("1 h ago"))
                                .icon(Icon::Phone),
                        ],
                    )
                    .pending("Not available yet"),
                ]),
            },
            Page {
                id: "power",
                title: "Power",
                icon: Icon::Power,
                summary: "Idle lock, suspend, power button",
                content: Content::Settings(&[
                    section("Automatic idle actions", &[
                        item("power.lock", "Lock when idle", Kind::Toggle(true))
                            .detail("Phones use the lock delay after the display turns off.")
                            .denial("power.idleLockEnabled"),
                        item("power.lockTimeout", "Idle lock timeout", Kind::Opens)
                            .from(Source::Own).only("power.lock", &[1]),
                        item("power.display", "Turn off display when idle", Kind::Toggle(true))
                            .denial("power.idleDpmsEnabled"),
                        item("power.displayTimeout", "Display-off timeout", Kind::Opens)
                            .from(Source::Own).only("power.display", &[1]),
                        item("power.suspend", "Sleep automatically", Kind::Toggle(false))
                            .denial("power.idleSuspendEnabled"),
                        item("power.suspendTimeout", "Sleep after the screen turns off", Kind::Opens)
                            .detail("Counted from when the screen turns off: by its timeout, the power button or the cover.")
                            .from(Source::Own).only("power.suspend", &[1]),
                        item("power.suspendMode", "Suspend mode", Kind::Opens)
                            .from(Source::Own),
                        item("power.idleInhibit", "Idle inhibition", Kind::Info(""))
                            .icon(Icon::Info)
                            .detail("Application activity such as video playback can temporarily pause these timers."),
                    ]),
                    section("Power button", &[
                        item("power.press", "Short press", Kind::Opens).from(Source::Own),
                        item("power.hold", "Long press", Kind::Opens)
                            .detail("The mobile shell uses the power menu.")
                            .from(Source::Own),
                    ]),
                    lead(&[item("power.reset", "Reset power preferences", Kind::Action("")).from(Source::Own)]),
                ]),
            },
            Page {
                id: "battery",
                title: "Battery",
                icon: Icon::Battery,
                summary: "Level, health",
                content: Content::Battery,
            },
            Page {
                id: "storage",
                title: "Storage",
                icon: Icon::Storage,
                summary: "Used and free space",
                content: Content::Storage,
            },
        ],
    },
    Group {
        title: "Personal",
        pages: &[
            Page {
                id: "you",
                title: "You",
                icon: Icon::Person,
                summary: "Account name, display name, profile photo",
                content: Content::Profile,
            },
            Page {
                id: "style",
                title: "Wallpaper and style",
                icon: Icon::Palette,
                summary: "Wallpaper, accent, glass",
                content: Content::Settings(&[
                    lead(&[
                        item("style.wallpaper", "Wallpaper", Kind::Opens)
                            .icon(Icon::Photos)
                            .from(Source::Own),
                        item(
                            "style.accent",
                            "Accent colour",
                            Kind::Choice(
                                &[
                                    "From wallpaper",
                                    "Lavender",
                                    "Blue",
                                    "Teal",
                                    "Green",
                                    "Amber",
                                    "Rose",
                                ],
                                0,
                            ),
                        )
                        .icon(Icon::Palette)
                        .from(Source::Own),
                        item("style.customAccent", "Custom accent colour", Kind::Opens)
                            .detail("The focused-window border follows the accent.")
                            .from(Source::Own),
                        item("style.resetAccent", "Reset custom accent colour", Kind::Action("")).from(Source::Own),
                    ]),
                    section(
                        "Application appearance",
                        &[
                            item(
                                "style.appTheming",
                                "Theme supported applications",
                                Kind::Toggle(true),
                            ).denial("appearance.applicationThemingEnabled"),
                            item("style.font", "Font family", Kind::Opens).from(Source::Own),
                            item("style.reapplyTheme", "Reapply application theming", Kind::Action(""))
                                .from(Source::Own).only("style.appTheming", &[1]),
                        ],
                    ).under("Application appearance"),
                    section(
                        "Material",
                        &[
                            item(
                                "style.material",
                                "Surfaces",
                                Kind::Segmented(&["Glass", "Blur", "Solid"], 0),
                            )
                            .from(Source::Words(
                                "appearance.transparencyMode",
                                &["glass", "blur", "off"],
                            )),
                            item("style.corners", "Roundness", Kind::Slider(0.5))
                                .from(Source::Range("appearance.cornerRadiusScale", 0.0, 2.0)),
                        ],
                    ),
                    section(
                        "Surface opacity and blur",
                        &[
                            item("style.islands", "Glass opacity", Kind::Slider(0.46))
                                .detail("The shell's panels and cards")
                                .from(Source::Range("appearance.glass.opacity", 0.0, 1.0))
                                .only("style.material", &[0]),
                            item("style.windows", "Window opacity", Kind::Slider(0.5))
                                .detail("Behind apps")
                                .from(Source::Range("appearance.glass.windowOpacity", 0.0, 1.0))
                                .only("style.material", &[0, 1]),
                            measured(
                                "style.frost", "Frost", "appearance.glass.blurSigma",
                                10.0, 0.0, 30.0, SliderUnit::Pixels,
                            )
                                .detail("How much glass blurs")
                                .only("style.material", &[0]),
                            measured(
                                "style.appPanelOpacity",
                                "Glass app-panel opacity",
                                "appearance.glass.appPanelOpacity",
                                0.4,
                                0.0,
                                1.0,
                                SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.blurThreshold",
                                "Blur opacity threshold",
                                "appearance.backdropBlurOpacityThreshold",
                                0.19,
                                0.0,
                                1.0,
                                SliderUnit::Percent,
                            ).only("style.material", &[0, 1]),
                            item(
                                "style.quality",
                                "Blur quality",
                                Kind::Segmented(&["Low", "Medium", "High", "Best"], 2),
                            )
                            .from(Source::Words(
                                "appearance.backdropBlurLevel",
                                &["shitty", "fast", "good", "best"],
                            ))
                            .only("style.material", &[1]),
                            item("style.panels", "Panel opacity", Kind::Slider(0.47))
                                .from(Source::Range("appearance.panelOpacity", 0.05, 1.0))
                                .only("style.material", &[1]),
                            item("style.cards", "Card opacity", Kind::Slider(0.44))
                                .detail("Notifications and tiles")
                                .from(Source::Range("appearance.cardOpacity", 0.0, 1.0))
                                .only("style.material", &[1]),
                        ],
                    ).under("Material effects"),
                    section(
                        "Glass colour",
                        &[
                            measured(
                                "style.glassSaturation", "Glass saturation", "appearance.glass.saturation",
                                1.0, 0.5, 2.0, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassTint", "Glass accent tint", "appearance.glass.tintStrength",
                                0.1, 0.0, 0.4, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassBrightness", "Glass brightness", "appearance.glass.brightness",
                                0.0, -0.2, 0.2, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                        ],
                    ).under("Material effects"),
                    section(
                        "Glass optics",
                        &[
                            measured(
                                "style.glassThickness", "Glass thickness", "appearance.glass.thickness",
                                25.0, 4.0, 48.0, SliderUnit::Pixels,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassRefraction", "Glass refraction", "appearance.glass.refraction",
                                0.2, 0.0, 1.0, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassDispersion", "Glass dispersion", "appearance.glass.dispersion",
                                0.0, 0.0, 1.0, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassBevel", "Glass bevel width", "appearance.glass.bevelWidthScale",
                                1.0, 0.25, 3.0, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassDepth", "Glass refraction depth", "appearance.glass.refractionDepthScale",
                                1.1, 0.25, 3.0, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                        ],
                    ).under("Material effects"),
                    section(
                        "Glass lighting",
                        &[
                            measured(
                                "style.glassLightAngle", "Glass lighting angle", "appearance.glass.lightAngle",
                                50.0, 0.0, 360.0, SliderUnit::Degrees,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassLightIntensity", "Glass light intensity", "appearance.glass.lightIntensity",
                                1.0, 0.0, 1.5, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassEdge", "Glass edge strength", "appearance.glass.edgeStrength",
                                1.0, 0.0, 1.5, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassRimWidth", "Glass rim width", "appearance.glass.rimWidth",
                                1.0, 0.5, 6.0, SliderUnit::Pixels,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassRimFalloff", "Glass rim falloff", "appearance.glass.rimFalloff",
                                1.1, 0.1, 3.0, SliderUnit::Decimal,
                            ).only("style.material", &[0]),
                            measured(
                                "style.glassOppositeLight", "Glass opposite light", "appearance.glass.oppositeLightStrength",
                                0.8, 0.0, 1.5, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                        ],
                    ).under("Material effects"),
                    section(
                        "Glass rendering",
                        &[
                            measured(
                                "style.glassQuality", "Glass rendering quality", "appearance.glass.quality",
                                1.0, 0.25, 1.0, SliderUnit::Percent,
                            ).only("style.material", &[0]),
                            item("style.resetGlass", "Reset glass effects", Kind::Action(""))
                                .from(Source::Own)
                                .only("style.material", &[0]),
                        ],
                    ).under("Material effects"),
                    section(
                        "Window appearance",
                        &[
                            item(
                                "style.focusBorder",
                                "Focused-window border",
                                Kind::Toggle(true),
                            )
                            .denial("appearance.focusedWindowBorderEnabled"),
                            measured(
                                "style.focusOpacity", "Focused-window opacity",
                                "appearance.focusedWindowOpacity",
                                1.0, 0.35, 1.0, SliderUnit::Percent,
                            ),
                            measured(
                                "style.unfocusedOpacity", "Unfocused-window opacity",
                                "appearance.unfocusedWindowOpacity",
                                1.0, 0.2, 1.0, SliderUnit::Percent,
                            ),
                        ],
                    ).under("Windows and pointer"),
                    section(
                        "Pointer",
                        &[
                            item("style.cursorTheme", "Cursor theme", Kind::Opens).from(Source::Own),
                            item("style.cursorImport", "Import cursor theme ZIP", Kind::Action("")).from(Source::Own),
                            item("style.cursorRemove", "Remove imported cursor theme", Kind::Opens).from(Source::Own),
                            measured(
                                "style.cursorSize", "Cursor size", "appearance.cursorSize",
                                32.0, 16.0, 64.0, SliderUnit::Pixels,
                            ),
                            item(
                                "style.clientCursor",
                                "Allow application cursors",
                                Kind::Toggle(true),
                            )
                            .denial("appearance.allowClientCursorSurfaces"),
                        ],
                    ).under("Windows and pointer"),
                    section(
                        "Motion",
                        &[
                            measured(
                                "style.animationDuration", "Animation duration", "animations.durationScale",
                                1.0, 0.5, 2.0, SliderUnit::Percent,
                            ),
                            measured(
                                "style.panelTravel", "Desktop panel travel", "animations.panelTravel",
                                32.0, 0.0, 96.0, SliderUnit::Pixels,
                            ),
                            item("style.closeEffect", "Window close effect", Kind::Choice(&["None", "Fade", "Implode", "Explosion"], 2))
                                .from(Source::Words("animations.windowCloseEffect", &["none", "fade", "implode", "explosion"])),
                            item("style.resetMotion", "Reset animations", Kind::Action("")).from(Source::Own),
                        ],
                    ).under("Motion"),
                    section(
                        "Lock screen",
                        &[
                            item("style.lockLayout", "Customize tablet lock screen", Kind::Opens)
                                .detail("Clock place and size, PIN pad side")
                                .from(Source::Own),
                            item(
                                "style.systemStatus",
                                "Show system status",
                                Kind::Toggle(true),
                            )
                            .denial("lockScreen.showSystemStatus"),
                            item(
                                "style.lockAnimation",
                                "Animate lock screen",
                                Kind::Toggle(true),
                            )
                            .denial("animations.animateLockScreen"),
                            item("style.clock", "Clock size", Kind::Slider(0.5))
                                .from(Source::Range("lockScreen.clockScale", 0.65, 1.4)),
                            item("style.dim", "Dim the wallpaper", Kind::Slider(0.0))
                                .from(Source::Range("lockScreen.dimAmount", 0.0, 0.85)),
                            item("style.blur", "Blur the wallpaper", Kind::Slider(0.0))
                                .from(Source::Range("lockScreen.blurRadius", 0.0, 32.0)),
                            item("style.resetLock", "Reset lock screen preferences", Kind::Action("")).from(Source::Own),
                        ],
                    ).under("Lock screen"),
                    lead(&[item("style.resetAppearance", "Reset appearance", Kind::Action("")).from(Source::Own)]),
                ]),
            },
            Page {
                id: "security",
                title: "Lock screen and security",
                icon: Icon::Lock,
                summary: "Screen lock, fingerprint",
                content: Content::Settings(&[
                    section(
                        "Unlocking",
                        &[
                            item(
                                "security.lock",
                                "Screen lock",
                                Kind::Choice(&["PIN", "Password"], 1),
                            )
                            .icon(Icon::Lock)
                            .from(Source::Own),
                            item("security.pin", "Change PIN", Kind::Opens)
                                .from(Source::Own)
                                .only("security.lock", &[0]),
                            item("security.fingerprint", "Fingerprint", Kind::Opens)
                                .icon(Icon::Fingerprint)
                                .from(Source::Own),
                            item(
                                "security.after",
                                "Lock after the screen goes off",
                                Kind::Choice(
                                    &[
                                        "Instantly",
                                        "5 seconds",
                                        "30 seconds",
                                        "1 minute",
                                        "5 minutes",
                                        "10 minutes",
                                        "Never",
                                    ],
                                    0,
                                ),
                            )
                            .from(Source::Own),
                        ],
                    ),
                    section(
                        "Lock screen",
                        &[
                            item("security.aod", "Always show the clock", Kind::Toggle(false))
                                .detail("Uses a little more battery")
                                .fixed(),
                        ],
                    ),
                ]),
            },
            Page {
                id: "privacy",
                title: "Privacy",
                icon: Icon::Shield,
                summary: "Camera, microphone",
                content: Content::Settings(&[
                    section(
                        "Access",
                        &[
                            item("privacy.camera", "Camera", Kind::Toggle(true))
                                .detail("For all apps")
                                .fixed(),
                            item("privacy.microphone", "Microphone", Kind::Toggle(true))
                                .detail("For all apps")
                                .fixed(),
                        ],
                    ),
                    section(
                        "Passwords",
                        &[item(
                            "privacy.reveal",
                            "Show letters as you type",
                            Kind::Toggle(true),
                        )
                        .fixed()],
                    ),
                ]),
            },
            Page {
                id: "location",
                title: "Location",
                icon: Icon::Location,
                summary: "GPS, apps that may use it",
                content: Content::Location,
            },
        ],
    },
    Group {
        title: "System",
        pages: &[
            Page {
                id: "environment",
                title: "Application environment",
                icon: Icon::Settings,
                summary: "Variables for all apps or individual apps",
                content: Content::Environment,
            },
            Page {
                id: "permissions",
                title: "Permissions",
                icon: Icon::Key,
                summary: "Keyboard shortcut access",
                content: Content::Permissions,
            },
            Page {
                id: "apps",
                title: "Apps",
                icon: Icon::Apps,
                summary: "Installed apps, defaults",
                content: Content::Apps(&[section(
                    "Defaults",
                    &[
                        item("apps.sms", "Messages", Kind::Choice(&["Messages"], 0))
                            .icon(Icon::Message)
                            .fixed(),
                        item("apps.phone", "Phone", Kind::Choice(&["Calls"], 0))
                            .icon(Icon::Phone)
                            .fixed(),
                    ],
                )]),
            },
            Page {
                id: "android",
                title: "Android (Droidloom)",
                icon: Icon::Android,
                summary: "Android apps, through Droidloom",
                content: Content::Android(&[lead(&[item(
                    "android.run",
                    "Run Android apps",
                    Kind::Toggle(true),
                )
                .detail("In their own windows, through Droidloom")
                .icon(Icon::Android)
                .from(Source::Own)])]),
            },
            Page {
                id: "language",
                title: "Language and keyboard",
                icon: Icon::Language,
                summary: "Language, keyboard",
                content: Content::Settings(&[
                    lead(&[item(
                        "language.system",
                        "Language",
                        Kind::Choice(&["System default", "English", "简体中文", "Русский"], 0),
                    )
                    .icon(Icon::Language)
                    .from(Source::Own),
                    item("language.reset", "Reset language preferences", Kind::Action("")).from(Source::Own)]),
                    section(
                        "Keyboard",
                        &[
                            item("language.correct", "Autocorrect", Kind::Toggle(false)).fixed(),
                            item(
                                "language.popup",
                                "Show the key you press",
                                Kind::Toggle(false),
                            )
                            .fixed(),
                            item("language.height", "Keyboard height", Kind::Slider(0.5)).fixed(),
                        ],
                    ),
                ]),
            },
            Page {
                id: "time",
                title: "Date and time",
                icon: Icon::Recent,
                summary: "Time zone, 24-hour clock",
                content: Content::Settings(&[
                    lead(&[
                        item("time.auto", "Set the time by itself", Kind::Toggle(true))
                            .detail("From the network")
                            .from(Source::Own),
                        item("time.date", "Date", Kind::Opens)
                            .from(Source::Own)
                            .only("time.auto", &[0]),
                        item("time.time", "Time", Kind::Opens)
                            .from(Source::Own)
                            .only("time.auto", &[0]),
                    ]),
                    section(
                        "Clock",
                        &[
                            item("time.zone", "Time zone", Kind::Opens)
                                .icon(Icon::Location)
                                .from(Source::Own),
                            item("time.24", "24-hour clock", Kind::Toggle(true))
                                .detail("17:45 rather than 5:45 PM")
                                .denial("localization.twentyFourHourClock"),
                        ],
                    ),
                    section(
                        "Region",
                        &[item("time.country", "Country or region", Kind::Opens)
                            .icon(Icon::Wifi)
                            .from(Source::Own)],
                    ),
                ]),
            },
            Page {
                id: "accessibility",
                title: "Accessibility",
                icon: Icon::Accessibility,
                summary: "Motion, text, contrast",
                content: Content::Hidden(&[
                    section(
                        "Seeing",
                        &[
                            item("accessibility.bold", "Bold text", Kind::Toggle(false)),
                            item(
                                "accessibility.contrast",
                                "Higher contrast",
                                Kind::Toggle(false),
                            )
                            .detail("Islands get less see-through"),
                        ],
                    ),
                    section(
                        "Motion",
                        &[
                            item("accessibility.motion", "Reduce motion", Kind::Toggle(false))
                                .detail("Screens change without moving"),
                            item(
                                "accessibility.hold",
                                "Touch and hold delay",
                                Kind::Segmented(&["Short", "Medium", "Long"], 0),
                            ),
                        ],
                    ),
                ]),
            },
            Page {
                id: "updates",
                title: "Updates",
                icon: Icon::Update,
                summary: "Denial 0.9 · up to date",
                content: Content::Hidden(&[
                    lead(&[
                        item("updates.state", "Up to date", Kind::Info("Denial 0.9"))
                            .detail("Checked today at 09:12")
                            .icon(Icon::Update),
                    ]),
                    section(
                        "Updates",
                        &[
                            item(
                                "updates.check",
                                "Check for updates",
                                Kind::Action("No new update"),
                            ),
                            item(
                                "updates.channel",
                                "Channel",
                                Kind::Segmented(&["Stable", "Beta", "Nightly"], 0),
                            ),
                            item("updates.wifi", "Download on Wi-Fi only", Kind::Toggle(true)),
                        ],
                    ),
                ]),
            },
            Page {
                id: "plugins",
                title: "Plugins",
                icon: Icon::Gridlines,
                summary: "What your desktop is built from",
                content: Content::Plugins,
            },
            Page {
                id: "developer",
                title: "Developer",
                icon: Icon::Code,
                summary: "Live UI development, builds, recovery",
                content: Content::Developer,
            },
            Page {
                id: "about-denial",
                title: "About Denial",
                icon: Icon::Info,
                summary: "Support and credits",
                content: Content::AboutDenial,
            },
            Page {
                id: "about",
                title: "About phone",
                icon: Icon::Device,
                summary: "Name, model, versions",
                content: Content::About,
            },
        ],
    },
];

/// Whether a monitor is plugged in beside the phone, as Settings last
/// found: its page is listed only then.
static MONITOR: AtomicBool = AtomicBool::new(false);

pub fn set_monitor(plugged: bool) {
    MONITOR.store(plugged, Ordering::Relaxed);
}

/// Whether Denial has something for a developer, as Settings last found:
/// live development installed or set up, or a shell other than the
/// packaged one.
static DEVELOPER: AtomicBool = AtomicBool::new(false);

pub fn set_developer(offered: bool) {
    DEVELOPER.store(offered, Ordering::Relaxed);
}

/// Whether this machine has Wi-Fi hardware, as Settings last found; so
/// until it has looked.
static WIFI: AtomicBool = AtomicBool::new(true);

pub fn set_wifi(present: bool) {
    WIFI.store(present, Ordering::Relaxed);
}

/// Whether the modem service runs here, as Settings last found; so until
/// it has looked.
static MOBILE: AtomicBool = AtomicBool::new(true);

pub fn set_mobile(present: bool) {
    MOBILE.store(present, Ordering::Relaxed);
}

/// Whether the home screen lists `page` now.
pub fn shown(page: &Page) -> bool {
    match page.content {
        Content::Hidden(_) => false,
        Content::Wifi => WIFI.load(Ordering::Relaxed),
        Content::Mobile => MOBILE.load(Ordering::Relaxed),
        // A hotspot shares over Wi-Fi.
        Content::Unavailable if page.id == "hotspot" => WIFI.load(Ordering::Relaxed),
        Content::Monitor(_) => MONITOR.load(Ordering::Relaxed),
        Content::Plugins => crate::plugins::command().is_some(),
        Content::Developer => DEVELOPER.load(Ordering::Relaxed),
        _ => true,
    }
}

/// The pages the home screen lists, in their groups' order.
pub fn listed() -> impl Iterator<Item = &'static Page> {
    GROUPS
        .iter()
        .flat_map(|group| group.pages)
        .filter(|page| shown(page))
}

/// The page with id `id`, if it opens.
pub fn page(id: &str) -> Option<&'static Page> {
    listed().find(|page| page.id == id && !matches!(page.content, Content::Unavailable))
}

/// The setting with key `key`.
pub fn find(key: &str) -> Option<&'static Item> {
    items()
        .map(|(_, _, item)| item)
        .find(|item| item.key == key)
}

pub static INPUT_DECK_CONTROLS: &[Section] = &[lead(&[
    measured("displays.deck-keyboard", "Keyboard height", "inputDeck.keyboardHeight", 0.48, 0.30, 0.65, SliderUnit::Percent),
    measured("displays.deck-width", "Touchpad width", "inputDeck.touchpadWidth", 0.62, 0.35, 1.0, SliderUnit::Percent),
    measured("displays.deck-height", "Touchpad height", "inputDeck.touchpadHeight", 1.0, 0.35, 1.0, SliderUnit::Percent),
    measured("displays.deck-speed", "Pointer speed", "inputDeck.pointerSpeed", 1.5, 0.25, 4.0, SliderUnit::Multiplier),
    item("displays.deck-scroll", "Natural scrolling", Kind::Toggle(true)).denial("inputDeck.naturalScroll"),
    item("displays.deck-cpu", "Show CPU chart", Kind::Toggle(false)).denial("inputDeck.showCpu"),
    item("displays.deck-gpu", "Show GPU charts", Kind::Toggle(false)).denial("inputDeck.showGpu"),
])];

/// Every setting on a listed page, with its page and section.
pub fn items() -> impl Iterator<Item = (&'static Page, &'static Section, &'static Item)> {
    listed().flat_map(|page| {
        let sections = match page.content {
            Content::Settings(sections)
            | Content::Apps(sections)
            | Content::Android(sections)
            | Content::Monitor(sections) => sections,
            Content::OutputConfiguration => INPUT_DECK_CONTROLS,
            Content::Wifi
            | Content::Mobile
            | Content::Location
            | Content::Bluetooth
            | Content::Battery
            | Content::Display
            | Content::Input
            | Content::Permissions
            | Content::Profile
            | Content::Environment
            | Content::Shortcuts
            | Content::Plugins
            | Content::Developer
            | Content::About
            | Content::AboutDenial
            | Content::Storage
            | Content::Hidden(_)
            | Content::Unavailable => &[],
        };
        sections
            .iter()
            .filter(|section| section.pending.is_none())
            .flat_map(move |section| section.items.iter().map(move |item| (page, section, item)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_start_with_their_page() {
        set_monitor(true);
        let mut keys = std::collections::HashSet::new();
        for (page, _, item) in items() {
            assert!(keys.insert(item.key), "{} twice", item.key);
            assert_eq!(item.key.split('.').next(), Some(page.id), "{}", item.key);
        }
    }

    #[test]
    fn defaults_are_options() {
        set_monitor(true);
        for (_, _, item) in items() {
            match item.kind {
                Kind::Segmented(options, start) | Kind::Choice(options, start) => {
                    assert!(start < options.len(), "{}", item.key)
                }
                Kind::Slider(start) => assert!((0.0..=1.0).contains(&start), "{}", item.key),
                _ => {}
            }
            if let (
                Source::Words(_, words),
                Kind::Segmented(options, _) | Kind::Choice(options, _),
            ) = (item.source, &item.kind)
            {
                assert_eq!(words.len(), options.len(), "{}", item.key);
            }
        }
    }

    #[test]
    fn own_choices_match_their_tables() {
        let options = |key: &str| match items().find(|(_, _, item)| item.key == key) {
            Some((
                _,
                _,
                Item {
                    kind: Kind::Choice(options, _),
                    ..
                },
            )) => options.to_vec(),
            _ => panic!("{key}"),
        };
        let accents: Vec<&str> = ACCENTS.iter().map(|(name, _)| *name).collect();
        assert_eq!(options("style.accent")[1..], accents[..]);
        let delays: Vec<&str> = LOCK_DELAYS.iter().map(|(name, _)| *name).collect();
        assert_eq!(options("security.after")[..delays.len()], delays[..]);
        assert_eq!(options("security.after")[delays.len()..], ["Never"]);
    }
}
