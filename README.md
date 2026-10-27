# denial-apps

Native apps for Denial devices, and the libraries they share. Written in Rust
with GPUI.

Device services and per-device configuration are in
[Phonewave](https://github.com/denialwm/phonewave).

## Layout

| Path | Contents |
|---|---|
| `apps/<name>/` | One app per directory, with its own README and device build scripts. |
| `libraries/<name>/` | Our crates shared by the apps. |

Everything is one Cargo workspace. The root `Cargo.toml` carries the release
profile and the `[patch.crates-io]` entries GPUI needs (zed's `async-process`,
`async-task` and `calloop`); a member's own patches are ignored.

## Apps

| Name | Purpose |
|---|---|
| [`messages`](apps/messages) | SMS app, on Phonewave's dmail, and its notifier |
| [`calls`](apps/calls) | Phone and contacts app; calls are mocked until dmail's Voice1 works |
| [`camera`](apps/camera) | Camera app: our own raw pipeline, burst photos, front and back cameras |
| [`settings`](apps/settings) | Phone settings: pages in groups, search, Islands controls; the settings are made up for now |
| [`sensors`](apps/sensors) | What the phone's sensors read, from Phonewave's steinerd: orientation, compass, level, gyroscope, ambient light, proximity |

## Libraries

| Name | Purpose |
|---|---|
| [`denial-contacts`](libraries/denial-contacts) | The contacts folder of vCard files, number matching as dmail does, inotify following |
| [`denial-look`](libraries/denial-look) | Denial's application look: dark and light palettes, floors, radii, motion, mobile layout tokens, live settings |
| [`denial-look-gpui`](libraries/denial-look-gpui) | The mobile materials as GPUI elements: island, glass group, chip, the text field, shared icons |

## Build

From an app's directory, `cargo run --release` on the host; the app's README
covers device builds. Output goes to `target/` here.

## Icons

Each app's launcher icon, `apps/<name>/org.denialwm.<Name>.png`, comes from
the [Yaru](https://github.com/ubuntu/yaru) icon theme (commit `7f18973e`),
whose icons are under
[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/). They keep
that license, not the one below. Calls, Camera, Messages, Settings and
Sensors are Yaru's `calls-app`, `camera-app`, `messaging-app`, `settings-app`
and `system-monitor-app` from `icons/src/fullcolor/default/apps/`. Each is the 256-unit drawing of its
source sheet, with the viewBox cropped to the drawing so it fills its square
as Android's icons do, rendered at 512 px with `rsvg-convert`. Messages and
Calls are recolored: every hex color with HSL saturation of at least 0.25
(except Yaru's `#ff0000` guides) has its hue turned by +75° with lightness times
0.58 for Messages (to green), and by +115° with lightness times 0.84 for
Calls (to blue).

On the phone, an app's `.desktop` file goes in `~/.local/share/applications`
and its icon in `~/.local/share/icons/hicolor/512x512/apps`; the `Icon=` key
names it.

## License

denial-apps is source-available, not open source. It uses the
[Sustainable Use License](LICENSE.md), the license n8n uses:

- Anyone may use and change it, including inside a company.
- Non-commercial projects may ship it, free of charge.
- Shipping it for a commercial purpose, such as on phones, in OS images or in
  paid apps and services, needs a separate license:
  doctor.logix@gmail.com.

[Denial](https://github.com/denialwm/denial),
[linux-perpetua](https://github.com/denialwm/linux-perpetua) and
[Phonewave](https://github.com/denialwm/phonewave) are open source.
