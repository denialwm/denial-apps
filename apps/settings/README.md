# Settings

Settings for Denial phones, written with GPUI. It is drawn to Denial's
mobile materials, Islands (denial/docs/MOBILE_APP_MATERIALS.md), as Messages
and Calls are. Denial's desktop settings app is a separate program.

The home screen lists the pages in one island per group (Connections,
Device, Personal, System), the group's name inside its island; the top group
holds the title and Search, and scrolls away with the list. Search looks
through the pages and the settings on them; a hit opens its page. A page
holds its settings in section islands, a chip over each titled one. A
setting is a toggle, a slider, a segmented choice, a choice picked in a
dialog on the gesture zone, a value to read, or an action that says what it
did on a chip. A slider claims the touch, so the page does not scroll under
the finger. Back and Escape run one Back action: the dialog closes, then
search ends, then a page goes back home, and at home the window closes.

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
on (operator, type, signal, roaming) or why there is none (SIM locked or
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

Language is real (`src/language.rs`): English or Simplified Chinese. A pick
sets Denial's `localization.locale`, which the shell and the native apps
follow at once; `LANG` in Denial's application environment, for the apps
Denial starts next; and the system's language, through systemd's localed.
The apps translate through `denial_look::words`, each with its own table of
words (`src/words.rs` here). The keyboard settings are not on the phone yet.

timedated, localed and hostnamed change things only with polkit's yes: on the Ace 3
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
window, set through droidloomd's `set_dpi`. The apps are the
launcher entries Droidloom exports (`X-Droidloom-Package`). An app's screen
opens and stops it, shows its version, dates and storage, clears its cache or
its storage, opens Android's App info for permissions and notifications, and
uninstalls it unless it is part of Android.

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
  none, since Denial's lock screen leaves the reader off without a PIN. The
  PIN opens Denial's fingerprint session, `sudo -n deniald
  --fingerprint-settings`, which adds and removes fingers; a PIN just set
  opens it at once. The device's sudo rule lets the session's user start
  that one command without a password. The session ends after five
  minutes or when the screen closes; the next change asks for the PIN
  again. The count on the Security page is fprintd's list. While the phone
  is locked Denial holds the reader.
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
  `alignmentY` when the finger lets go; each screen's own placement goes. A
  new wallpaper starts centred at 100 percent. A tap
  rewrites the shell's `$XDG_STATE_HOME/denial/wallpaper`, which the shell
  follows, and when `wallpaper-depth` is installed starts the lock screen's
  depth mask of the new picture, as the shell's picker does.
- In Settings' own code (`src/settings/system.rs`): date and time, the
  language, Android starting.
- Not on the phone yet: the keyboard, default apps, the ringtone, vibrating for calls, touch sounds,
  always showing the clock and the privacy switches. They show faint, on
  their values, and cannot be changed. "Recently sent" says it is not
  available yet.
- Made up: the rest. Changing one reaches nothing on the phone; the values
  are kept in `$XDG_STATE_HOME/denial/settings/values.json`
  (`src/values.rs`), so they stay across restarts.

`settings <page>` opens on a page by its id (`wifi`, `display`, ...). One
Settings runs at a time: it owns `org.denialwm.Settings` on the session bus,
and a second `settings <page>` hands the page and its activation token to it
through `Open` and exits.

## Dependencies

- `libraries/denial-look` and `libraries/denial-look-gpui`, as in Messages.
- The local gpui-touch checkout, as in Messages (see its README).

## Build

On the host:

```sh
cargo run --release
```

For the Ace 3, cross-built against its sysroot in
`/mnt/development/denial-os/cross/sysroot`:

```sh
./build-aston.sh
```

Both write to `target/` at the denial-apps root.

## Testing on a phone

Ignored tests in `src/wifi.rs` talk to the phone's real manager; build them
with `cargo test --release --target aarch64-unknown-linux-gnu --no-run` and
the environment of `build-aston.sh`, copy the test binary over and run it as
root:

- `reads_the_system` prints what the manager reports.
- `reads_dmail` prints what dmail reports.
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
