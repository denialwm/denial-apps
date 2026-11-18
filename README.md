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

Everything but Settings is one Cargo workspace. The root `Cargo.toml` carries
the release profile and the `[patch.crates-io]` entries GPUI needs (zed's
`async-process`, `async-task` and `calloop`); a member's own patches are
ignored. Settings is a workspace of its own, with its own `Cargo.lock` and
the same profile and patches in `apps/settings/Cargo.toml`: Denial builds it
for every distribution from this repository and gpui-touch alone, and Camera
needs Phonewave beside them. Keep the two in step when a patch or the
profile changes.

## Apps

| Name | Purpose |
|---|---|
| [`messages`](apps/messages) | SMS app, on Phonewave's dmail, and its notifier |
| [`calls`](apps/calls) | Phone and contacts app, on Phonewave's dmail |
| [`camera`](apps/camera) | Camera app: our own raw pipeline, burst photos, front and back cameras |
| [`settings`](apps/settings) | Denial's Settings and Welcome, on the desktop and the phones; Denial builds and packages it |
| [`sensors`](apps/sensors) | What the phone's sensors read, from Phonewave's steinerd: orientation, compass, level, gyroscope, ambient light, proximity |

## Libraries

| Name | Purpose |
|---|---|
| [`denial-contacts`](libraries/denial-contacts) | The contacts folder of vCard files, number matching as dmail does, inotify following |
| [`denial-look`](libraries/denial-look) | Denial's application look: dark and light palettes, floors, radii, motion, mobile layout tokens, live settings |
| [`denial-look-gpui`](libraries/denial-look-gpui) | The mobile materials as GPUI elements: island, glass group, chip, the text field, shared icons |

## Build

From an app's directory, `cargo run --release` on the host. Phones get the
apps as packages from Phonewave's `denial-apps` recipe:
`phonewave/tools/phone-install HOST denial-apps` from the workspace root tries
them on one. Settings comes with the `denial` package instead; see its
README. Output goes to `target/` here.

## Icons

Each app's launcher icon, `apps/<name>/org.denialwm.<Name>.png`, comes from
the [Yaru](https://github.com/ubuntu/yaru) icon theme (commit `7f18973e`),
whose icons are under
[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/). They retain
that license. Calls, Camera, Messages, Settings and
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

The original denial-apps source code is licensed under
[GPL-3.0-or-later](LICENSE.md), like Denial. Bundled third-party components
retain their licenses and attribution notices. The Yaru icons are
[CC BY-SA 4.0](LICENSES/CC-BY-SA-4.0.txt), as described above. The shared
text input includes code adapted from Zed's GPUI example under
[Apache 2.0](LICENSES/Apache-2.0.txt), with attribution in
[libraries/denial-look-gpui/src/input.rs](libraries/denial-look-gpui/src/input.rs).
