# Settings

Denial's Settings, on the desktop and the phones, written with GPUI. It is
drawn to Denial's mobile materials, Islands
(denial/docs/MOBILE_APP_MATERIALS.md), as Messages and Calls are. The same
program is Welcome, the desktop's first-start tour.

The home screen lists the pages in one island per group (Connections,
Device, Personal, System), the group's name inside its island; the top group
holds the title and Search, and scrolls away with the list. Search looks
through the pages and the settings on them; a hit opens its page. A page
holds its settings in section islands, a chip over each titled one. A
setting is a toggle, a slider, a segmented choice, a choice picked in a
dialog on the gesture zone, a value to read, or an action that says what it
did on a chip. A set of sections the catalog puts under a disclosure is one card:
its row opens it, and its sections show as panels inside the card, each
under its name. What is done to a whole section (reading it again, dropping its changes,
going back to Denial's own settings) is a small icon button at the end of
the section's heading; under the pointer it says what it does, and it is
faint while there is nothing for it to do. Where a page's changes wait to
be carried out (the displays, the physical keyboard) an Apply button shows
in the window's bottom right corner, and only then, so it is seen
arriving. A slider is a bar filled in the accent up to
its handle; it
claims the touch, so the page does not scroll under the finger. A segmented
choice slides its fill to the picked option on the switch's spring. The
list leaves out what the machine has nothing of: Wi-Fi and Hotspot without
Wi-Fi hardware, Mobile network without the modem service. Back and Escape run one Back action: the dialog closes, then
search ends, then a page goes back home, and at home the window closes.

Application environment is laid out as it was in Denial's own Settings: a
note on what the overrides reach, then, in a page at least 640 wide, the
applications at the left with a field that narrows them and the picked
one's cards beside them: its name with Clear, the card that adds or hides
a variable, and its configured overrides, each with an edit and a remove
button. A narrower page picks the application in a dialog and stacks the
same cards.

In a window at least 800 wide the list stays at the left, 320 wide, and the
page shows beside it, no wider than 960 so a row's value stays near its
name, in the middle of what the window leaves it. The search field is
always over the list there. A row of the list is a smaller tile and the
page's name, with a second line only for what Settings found (the connected
device, the battery's level); the shown page's row is filled and marked in
the accent, and a row under the pointer takes a faint fill. A section's
name there is a heading over its island, small capitals in the accent
text, on the page and on the list alike; where the list is the whole
window a page's sections are named on chips and the list's groups inside
their islands. A chip still says how things stand: nothing found, still
waiting. A page has no title there, since its row of the list is marked;
what a page opens, such as a saved network, keeps its name and its back
button. Each pane shows a thin
bar at its right, while the pointer is over it, that says how far it is
scrolled.
A page does not slide there: the one that leaves fades out, then the
islands of the one that arrives rise in, the nearest first
(`motion::OUT`, `IN`, `IN_STEP`). In both layouts a switch's knob slides
and the selection's fill fades on a spring that settles in about 200 ms,
and what a row opens fades in. All of it runs on Denial's animation
duration scale; at zero nothing moves.

Wi-Fi is real (`src/wifi.rs`, `src/settings/networks.rs`). It talks to
NetworkManager when its name is on the system bus, since NetworkManager may
drive iwd itself, else to iwd directly, as Denial's shell decides; it
switches when either starts or stops. The manager is read whole after every
change it signals, and the Wi-Fi screens ask it to scan every 10 s while
they are shown, since managers keep only what their last scan found. The
page has the switch, the network the phone is on, and the networks in
range; a tap joins one, asking for the password when it takes one, and asks
again after a wrong one. A saved network's screen has Connect or
Disconnect, Forget (confirmed), the connection's details, and how it is
joined: by itself or not, metered, and the hardware address
(NetworkManager only). Hidden networks are added by name. Enterprise
(802.1X) networks cannot be joined yet. Changes need polkit's yes, which
NetworkManager gives the user's seat session: Settings started from Denial
has it, one started over SSH does not.

Mobile network is real too (`src/mobile.rs`, `src/settings/cellular.rs`):
it reads dmail, Phonewave's modem service (`org.dmail` on the system bus),
and follows its property changes. The page shows the network the phone is
on (operator, type, signal, roaming, VoLTE working, starting or off) or why there is none (SIM locked or
blocked, no SIM, searching, the modem stopped), the Mobile data switch, which
SIM slot is used, and the network's details. A locked SIM is unlocked with
its PIN in a dialog, which comes back with the tries left after a wrong one.
Below them, what dmail knows when something is wrong: packet service and the
network's last refusal; the session's APN, address, DNS, MTU, how long it has
been up and its traffic (read from sysfs every two seconds while the page
shows); how many sessions the network ended and why the last one went. Then
three ways back, smallest first: reconnect mobile data (`Data1.Reconnect`),
restart the radio (`Modem1.RestartRadio`) and start the modem over
(`Modem1.Restart`, also the way out of "The modem stopped").
The bus is open to root and the `dmail` group only. dmail has no roaming,
network type, APN or airplane mode settings yet, so the page has none.

Location is real (`src/location.rs`, `src/settings/position.rs`). Its GPS
part reads dmail's `org.dmail.Location1` and follows its changes: where the
GPS is (off, looking for satellites and how many it hears, located and how
closely), a "Locate this device" switch, and the position found (latitude,
longitude, accuracy, speed and direction when moving, satellites used and
heard, the position's time). dmail runs the engine only while a client
asks: the switch calls `Location1.Start`, and Settings calls `Stop` when it
goes off or the page closes; dmail also lets go when Settings leaves the
bus, and Settings asks again when dmail restarts. Without dmail, or with a
modem that has no location engine, the page says there is no GPS. Below,
each app's answer to Denial's location question (Ask, Allow, Deny), from
the portal permission store, table `denial`, entry `location`: the entry
denial-portal's GeoClue agent reads and writes, edited like those on the
Permissions page.

Bluetooth is real as well (`src/bluetooth.rs`, `src/settings/devices.rs`),
through BlueZ (`org.bluez`). The page has the switch, the phone's name
(renamed in a dialog), the paired devices (a tap connects or disconnects
one, its gear opens it) and the named devices nearby (a tap pairs, trusts
and connects). Discovery runs while a Bluetooth screen is shown and stops
after; the phone is not made visible to others. A paired device's screen
has Connect or Disconnect, Forget (confirmed), its type, battery and
address. Settings serves its own pairing agent on the connection it pairs
from, not as the default agent, so BlueZ asks it only about pairings started
here: comparing a code, typing a PIN or passkey, or showing one to type on
the other device. Pairings another device starts go to the default agent,
Denial's shell.

Display is real (`src/display.rs`, `src/settings/panel.rs`), through
Denial's control socket (`$DENIAL_SOCKET`, else
`$XDG_RUNTIME_DIR/denial/control.sock`; Denial's
docs/protocol/control-v1.md). The page has the panel's brightness, the dark
or light appearance, when the idle screen turns off (from 30 seconds, or
never), and the refresh rate. Appearance and timeout live in Denial's
settings document: Settings changes them with `settings.document.apply` and
follows them with `settings.document.subscribe`. Brightness and the refresh
rate are read again every second while the page shows, since the shade
changes them too. A new refresh rate is applied with Denial's confirmation
timeout: a dialog asks to keep it, and Denial goes back by itself after 15 s
if nobody answers, so a rate the panel cannot show does not leave the
screen dark.

Battery is real (`src/battery.rs`, `src/settings/power.rs`), through UPower
(`org.freedesktop.UPower`), which starts when first asked. Only batteries
that report the energy they hold when full count, so a firmware battery
that reports a level and nothing else is left out. The page has the level
and what the battery is doing (charging and until when, or the time left),
then its health, charge cycles, temperature and power.

About phone is real (`src/about.rs`, `src/settings/phone.rs`): the model
and Denial's version (deniald's `denial.version`) at the top, then the
phone's name, its number (dmail's `Modem1.Number`, as the SIM holds it), its
Wi-Fi and Bluetooth addresses, storage used, the processor, memory and
panel, and the system, kernel and up time, read from the kernel's files
each time the page opens. The name is systemd's pretty hostname
(`src/name.rs`, `org.freedesktop.hostname1`), which Droidloom also gives
Android as its device name. A rename, here or on the Bluetooth page, sets it
and the Bluetooth adapter's name together.

Date and time is real (`src/clock.rs`, `src/settings/system.rs`), through
systemd's timedated (`org.freedesktop.timedate1`): setting the time from the
network (NTP), and while that is off the date and the time, typed in a
dialog; the time zone, picked by a city on a screen of their own; and the
24-hour clock, Denial's `localization.twentyFourHourClock`, which the shell's
clocks follow. The page reads timedated again every second while it shows.

The country or region on the same page is the one whose rules the Wi-Fi
radios follow (`src/country.rs`): wireless-regdb's
`/etc/conf.d/wireless-regdom`, which its udev rule applies whenever the Wi-Fi
stack loads. The `wifi-country` helper (`src/bin/wifi-country.rs`) writes it
and applies it at once with `iw`; Settings runs it through pkexec, under the
polkit action in `packaging/`. The list is every country the regulatory
database knows, in English and Chinese, searchable by name or code;
`tools/countries.py` writes it from wireless-regdb and iso-codes. The time
zone's country comes first, and picking a time zone while no country is set
sets that country too.

Language is real (`src/language.rs`): English or Simplified Chinese. A pick
sets Denial's `localization.locale`, which the shell and the native apps
follow at once; `LANG` in Denial's application environment, for the apps
Denial starts next; and the system's language, through systemd's localed.
The apps translate through `denial_look::words`, each with its own table of
words (`src/words.rs` here). The keyboard settings are not on the phone yet.

timedated, localed and hostnamed change things only with polkit's yes: on the 12R
Phonewave's `48-phone-settings.rules` gives it to the phone's owner at the
phone; elsewhere Denial's polkit agent asks for the password.

Storage is real: the space used and free on the root filesystem, then what
uses it, the home folder (counted file by file, on its own filesystem only)
and the rest as System.

Apps lists every application entry in the XDG data directories that Denial
would show (`src/apps.rs`), with its icon and name; default apps are not on
the phone yet.

Android is real (`src/droidloom.rs`, `src/settings/android.rs`). Without
Droidloom's user unit in /usr/lib, the page and the home screen say
Droidloom is not installed, and the page holds nothing else. Its switch
starts Android now and with each session: the user's
`graphical-session.target.wants` link to Droidloom's user unit. Restart
restarts that unit. How Android runs comes from droidloomd's control socket
(`/run/droidloom/control.sock`, one JSON request and one JSON answer), read
every two seconds while an Android screen or the home screen shows: ready or
starting, time running, how long it took to start, memory without the file
cache and swapped out, the share of the processor since the last reading,
processes, app processes and threads, Android's version, the model and name
Android gives apps, and Droidloom's release. Droidloom's unit is read with it
over the session bus. When the unit failed, the page and the home screen say
so, and the page shows the unit's last journal line from that run
(`_SYSTEMD_INVOCATION_ID`); when the cell stopped on its own, droidloomd's
reason. Display size draws Android apps at 85, 100, 115 or 130 percent of
the size of Denial's own: 160 dpi times the scale Denial gives the Settings
window, set through droidloomd's `set_dpi`, and the same size per unit of
scale for every screen in Droidloom's `$XDG_CONFIG_HOME/droidloom/density-v1.json`
(`dpiPerScale`), which Droidloom watches; with no size picked Droidloom
draws them at Default. The apps are the
launcher entries Droidloom exports (`X-Droidloom-Package`). An app's screen
opens and stops it, shows its version, dates and storage, clears its cache or
its storage, opens Android's App info for permissions and notifications, and
uninstalls it unless it is part of Android.

Monitor is listed only while deniald shows an enabled screen that is not
built in (`DSI-`, `eDP-`, `LVDS-`), asked every two seconds; it leaves the
page when the monitor goes. Scale and Refresh rate set that screen's scale
and rate through `outputs.apply` and ask to keep them, as the panel's refresh
rate does; rates of its current resolution that round to the same hertz show
once. Android apps
sets the monitor's own size in Droidloom's density file (`outputs`, by
connector); without one it takes every screen's. Windows and Wallpaper
are Denial's `layout.windowLayout` and the monitor's entries in the shell's
wallpaper file (`outputs`, `outputAlignments`, `outputZooms`).

Accessibility and Updates are kept in the catalog but not listed.

Hotspot is listed but not on the phone yet: its row shows faint and opens
nothing.

The other pages are tables in `src/catalog.rs`; each setting there says
where its value lives (`src/settings/bound.rs`):

- In Denial's settings document, changed with `settings.document.apply`
  and followed like the display's: Do not disturb, notifications on the
  lock screen (all or none), waking the screen for one, keyboard vibration,
  fingerprint unlock, the lock's delay once the screen goes off
  (`power.screenOffLockDelaySeconds`; Never turns `power.idleLockEnabled`
  off),
  the accent, the material and what it takes (glass opacities and frost,
  or blur quality and panel and card opacity), roundness and the lock
  screen's clock and wallpaper.
- The media volume, through Denial's `audio.get` and `audio.set`, read again
  every second while the Sound page shows. Ring and Alarms show the same
  level, faint: the phone has one volume for now.
- The screen lock: PIN or password, through `deniald --lock-pin` (the
  compositor that started Settings names itself in
  `DENIAL_COMPOSITOR_BINARY`). A new PIN has 6 digits, typed twice. The
  first PIN, or one forgotten, takes the account password; Change PIN takes
  the PIN set now; going back to the password takes the password. A PIN set
  before with 4 to 12 digits still works until it is changed.
- The fingers (`src/fingerprint.rs`), on a screen of their own behind the
  PIN: the Fingerprint row asks for it, or sets one first when there is
  none, since Denial's lock screen leaves the reader off without a PIN.
  `deniald --lock-pin check` proves the PIN; a PIN just set counts at once.
  Settings then adds and removes fingers through fprintd itself, as the
  session's user: the denial package's polkit rule lets the user at the
  phone change their own fingers without the account password. Where a
  Gatekeeper keeps the PIN, Denial's root session, `sudo -n deniald
  --fingerprint-settings`, adds them instead, since the reader wants the
  PIN proven to it. Either way the PIN is asked again after five minutes or
  once the screen closes. The count on the Security page is fprintd's list.
  While the phone is locked Denial holds the reader.
- The wallpaper (`src/wallpaper.rs`): Denial's own and the pictures in the
  wallpaper folder (`$DENIAL_WALLPAPER_DIR`, else `~/Pictures/Wallpapers`, as
  the shell has it), three to a row on a screen of their own, from
  thumbnails kept in `$XDG_CACHE_HOME/denial/settings/wallpapers`. Under
  them, Wallhaven's (`src/wallhaven.rs`), asked as the shell's picker asks:
  safe for work, the month's toplist when the field is empty, else the best
  matches, at least the screen's size, the screen's shape first, with
  `WALLHAVEN_API_KEY` when set. More reads the next page; Wallhaven's small
  pictures are kept in `$XDG_CACHE_HOME/denial/settings/wallhaven`. A tap
  downloads the picture into the wallpaper folder as `wallhaven-<id>`, the
  shell's name for it, showing the share received, then sets it. Above them
  all, the shown wallpaper's placement: a small screen with the picture
  placed in it (from a whole copy kept in the same cache), and sliders for
  its zoom (100 to 400 percent, enlarged about the point its position
  names, past covering the screen) and its position left and right, up and
  down. The shell's file takes them as `zoom`, `alignmentX` and
  `alignmentY` when the finger lets go; the built-in panel's own placement
  goes, and each monitor keeps its own. A
  new wallpaper starts centred at 100 percent. A tap
  rewrites the shell's `$XDG_STATE_HOME/denial/wallpaper`, which the shell
  follows, and when `wallpaper-depth` is installed starts the lock screen's
  depth mask of the new picture, as the shell's picker does.
- The plugins (`src/plugins.rs`, `src/settings/plugins.rs`), listed where
  `denial-plugins` is installed (`$DENIAL_PLUGINS_BINARY`, else in `PATH`).
  The manager decides what runs, why, what failed and what can be done now;
  the page shows what `denial-plugins status --json` and `list --json` say
  and works nothing out (Denial's docs/PLUGIN_MANAGER.md, section 23). It
  has what the desktop runs and why, the request that runs with its step
  and time, the last failure with its message and log, and Apply, Switch
  now, Undo and Standard desktop as the manager allows each. A plugin's
  switch saves the selection at once; the desktop changes with Apply. A
  plugin is added from a folder or a Git link, and Update plugins applies
  with their newest revisions. Plugins of the community list that were
  never added have a section of their own, and Refresh community plugins
  reads that list again. Use Denial defaults makes the selection the
  plugins Denial comes with, and only them. What a build took in because a
  selected plugin needs it is listed as included automatically. Recent
  activity shows the last ten requests (`denial-plugins history --json`)
  and why one failed; it is read only while it shows. Every sentence comes
  with a code: Settings
  words the codes it knows and shows the manager's English for the rest.
  The page reads again every 600 ms while a request runs, else every 5 s,
  and only while it shows. Plugin support is prepared once, when the page
  first opens and it is missing.
- Developer (`src/developer.rs`, `src/settings/developer.rs`), listed when
  the running Denial answers `ui.get` on its control socket: the mode the
  shell runs in with its status, progress and error, the switch to live UI
  development, the source workspace (`ui.workspace.set`, or `denialctl
  --json ui setup` to create one), reloading when files change, hot reload
  and hot restart, building and activating an optimized shell, reverting to
  the last one that worked, restoring the official one, the generation, the
  Dart VM service address (a tap copies it) and the diagnostics. It reads
  the state every second while it shows.
- In Settings' own code (`src/settings/system.rs`): date and time, the
  language, Android starting.
- Not on the phone yet: the keyboard, default apps, the ringtone, vibrating for calls, touch sounds,
  always showing the clock and the privacy switches. They show faint, on
  their values, and cannot be changed. "Recently sent" says it is not
  available yet.
- Made up: the rest. Changing one reaches nothing on the phone; the values
  are kept in `$XDG_STATE_HOME/denial/settings/values.json`
  (`src/values.rs`), so they stay across restarts.

`settings <page>` or `settings --page=<page>` opens on a page by its id
(`wifi`, `display`, ...). One Settings runs at a time: it owns
`org.denialwm.Settings` on the session bus, and a second `settings <page>`
hands the page and its activation token to it through `Open` and exits.

## Welcome

`settings --welcome` is Welcome, the tour a desktop opens on its first start:
the same program as an app of its own, `org.denialwm.Welcome`, with its own
window and launcher entry (`src/settings/welcome.rs`). Five steps: a greeting,
the shortcuts as they are bound now, the window layout, the look, and how to
support Denial. The layout and the look change the desktop as they are
picked, through the settings their pages use.

Finishing the tour, or leaving it at its first step, writes
`$XDG_CONFIG_HOME/denial/welcome` and closes it (`src/welcome.rs`). Denial
starts `settings --welcome --autostart` with every desktop session; with
`--autostart` it opens only while that file is missing. Remove the file to
see the tour on the next start again. The launcher's entry always opens it.

## Older glibc

Denial packages Settings for systems whose glibc is older than the one it is
built on, down to 2.39. `src/glibc.rs` holds the math functions newer glibc
versions gave new symbol versions, so the program never asks glibc for them.
Denial's packaging checks the baseline; a function it names goes there.

## Dependencies

- `libraries/denial-look` and `libraries/denial-look-gpui`, as in Messages.
- The local gpui-touch checkout, as in Messages (see its README).

## Build

On the host:

```sh
cargo run --release
```

This is a Cargo workspace of its own, with its own `Cargo.lock`: run Cargo
from this directory. It writes to `target/` at the denial-apps root, as the
other apps do.

Denial builds and packages it. `denial/settings/SOURCE_LOCK.json` names the
commit of this repository and of gpui-touch that Denial builds, for every
distribution and for the phones; the `denial` package installs the binary as
`/usr/lib/denial-apps/settings`, with `/usr/bin/denial-settings` naming it.
A change here reaches a release once it is pushed and that lock names its
commit. To build what is on disk the way Denial does, without moving the
lock:

```sh
DENIAL_SETTINGS_SOURCE_ROOT=/mnt/development/denial-os denial/tools/denial-settings build
```

`denial/settings/README.md` has the rest.

## Testing on a phone

Ignored tests in `src/wifi.rs` talk to the phone's real manager; build them
with `cargo test --release --target aarch64-unknown-linux-gnu --no-run` in
the environment of Phonewave's `image/cross.sh` (source it first), copy the
test binary over and run it as root:

- `reads_the_system` prints what the manager reports.
- `reads_dmail` prints what dmail reports.
- `reads_location` asks dmail for location for 40 s and prints what it
  reports; run it as a user in the `dmail` group.
- `reads_bluez` prints what BlueZ reports; it runs on any machine with
  BlueZ and changes nothing.
- `reads_upower` prints the battery as UPower reports it.
- `reads_facts` prints what About reads from the kernel; it runs on any
  machine.
- `reads_denial` prints the panel, its brightness, the display settings and
  the volume; run it as the session's user, with `XDG_RUNTIME_DIR` set.
- `reads_the_lock` prints the lock PIN's length and the enrolled fingers,
  as the session's user.
- `writes_denial` writes the brightness, appearance and timeout back as they
  are, through deniald.
- `changes_a_saved_network_and_back`, with `WIFI_TEST_NETWORK=<name>`,
  changes how that saved network is joined and puts it back.
- `leaves_and_rejoins` disconnects from the current network and joins it
  again.

## Renderer diagnostics

Set `DENIAL_SETTINGS_LOG` to a writable file to keep Settings' and the
renderer's information messages, and every warning and error, when a
launcher discards stderr. A new log file has mode 0600; an existing one is
appended to. Without the variable Settings logs nothing.
