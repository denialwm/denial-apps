//! What Settings lists: its pages, in groups on the home screen, and the
//! settings on each page, in sections. Wi-Fi, the mobile network,
//! Bluetooth, the display and the battery are real and have screens of
//! their own. A setting on a page says where its value lives (`Source`):
//! in Denial's settings document, in Denial's volume, worked out by
//! Settings itself, or made up for now, kept by the app (`values`) and
//! reaching nothing on the phone.

use denial_look_gpui::Icon;

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
    /// Bluetooth, through BlueZ.
    Bluetooth,
    /// The battery, through UPower.
    Battery,
    /// The display, through deniald.
    Display,
    /// What the phone is, from the kernel, BlueZ, dmail and deniald.
    About,
    /// The storage: used and free, and what uses it.
    Storage,
    /// Settings in sections, then the apps installed.
    Apps(&'static [Section]),
    /// Android through Droidloom: the switch in sections, then how it runs
    /// and its apps.
    Android(&'static [Section]),
    /// Written, but shown nowhere until the phone has it.
    Hidden(#[expect(dead_code, reason = "shown once the phone has it")] &'static [Section]),
    /// Listed, but not on the phone yet: its row shows faint and opens
    /// nothing.
    Unavailable,
}

/// Settings that belong together: one island, under a chip when titled.
pub struct Section {
    pub title: Option<&'static str>,
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
        only: None,
    }
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
    }
}

/// A section with no chip over it: a page's first, main switch.
const fn lead(items: &'static [Item]) -> Section {
    Section {
        title: None,
        items,
        pending: None,
    }
}

impl Section {
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
                    ]),
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
                            item("style.islands", "Glass opacity", Kind::Slider(0.46))
                                .detail("The shell's panels and cards")
                                .from(Source::Range("appearance.glass.opacity", 0.0, 1.0))
                                .only("style.material", &[0]),
                            item("style.windows", "Window opacity", Kind::Slider(0.5))
                                .detail("Behind apps")
                                .from(Source::Range("appearance.glass.windowOpacity", 0.0, 1.0))
                                .only("style.material", &[0, 1]),
                            item("style.frost", "Frost", Kind::Slider(0.7))
                                .detail("How much glass blurs")
                                .from(Source::Range("appearance.glass.blurSigma", 0.0, 30.0))
                                .only("style.material", &[0]),
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
                            item("style.corners", "Roundness", Kind::Slider(0.5))
                                .from(Source::Range("appearance.cornerRadiusScale", 0.0, 2.0)),
                        ],
                    ),
                    section(
                        "Lock screen",
                        &[
                            item("style.clock", "Clock size", Kind::Slider(0.5))
                                .from(Source::Range("lockScreen.clockScale", 0.65, 1.4)),
                            item("style.dim", "Dim the wallpaper", Kind::Slider(0.0))
                                .from(Source::Range("lockScreen.dimAmount", 0.0, 0.85)),
                            item("style.blur", "Blur the wallpaper", Kind::Slider(0.0))
                                .from(Source::Range("lockScreen.blurRadius", 0.0, 32.0)),
                        ],
                    ),
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
                summary: "Camera, microphone, location",
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
                            item("privacy.location", "Location", Kind::Toggle(true))
                                .icon(Icon::Location)
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
        ],
    },
    Group {
        title: "System",
        pages: &[
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
                        Kind::Choice(&["English", "简体中文"], 0),
                    )
                    .icon(Icon::Language)
                    .from(Source::Own)]),
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
                id: "about",
                title: "About phone",
                icon: Icon::Info,
                summary: "Name, model, versions",
                content: Content::About,
            },
        ],
    },
];

/// The pages the home screen lists, in their groups' order.
pub fn listed() -> impl Iterator<Item = &'static Page> {
    GROUPS
        .iter()
        .flat_map(|group| group.pages)
        .filter(|page| !matches!(page.content, Content::Hidden(_)))
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

/// Every setting on a listed page, with its page and section.
pub fn items() -> impl Iterator<Item = (&'static Page, &'static Section, &'static Item)> {
    listed().flat_map(|page| {
        let sections = match page.content {
            Content::Settings(sections) | Content::Apps(sections) | Content::Android(sections) => {
                sections
            }
            Content::Wifi
            | Content::Mobile
            | Content::Bluetooth
            | Content::Battery
            | Content::Display
            | Content::About
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
        let mut keys = std::collections::HashSet::new();
        for (page, _, item) in items() {
            assert!(keys.insert(item.key), "{} twice", item.key);
            assert_eq!(item.key.split('.').next(), Some(page.id), "{}", item.key);
        }
    }

    #[test]
    fn defaults_are_options() {
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
