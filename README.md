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

## Libraries

| Name | Purpose |
|---|---|
| [`denial-contacts`](libraries/denial-contacts) | The contacts folder of vCard files, number matching as dmail does, inotify following |
| [`denial-look`](libraries/denial-look) | Denial's application look: dark and light palettes, floors, radii, motion, mobile layout tokens, live settings |
| [`denial-look-gpui`](libraries/denial-look-gpui) | The mobile materials as GPUI elements: island, glass group, chip, the text field, shared icons |

## Build

From an app's directory, `cargo run --release` on the host; the app's README
covers device builds. Output goes to `target/` here.

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
