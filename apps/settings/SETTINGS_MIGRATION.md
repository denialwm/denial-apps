# Unifying Denial Settings in GPUI

This is a source inventory and migration proposal. “Desktop only” means the control configures desktop shell behavior, including on a tablet or external monitor. “Mobile only” means the control configures mobile shell behavior or telephony. “Shared” covers both presentations, with hardware and service checks where applicable. A narrow window can still open desktop controls. A wide tablet window can still show mobile controls.

## Desktop only

| Area | Controls | Current GPUI coverage |
| --- | --- | --- |
| Window layout and workspaces | Stacking, Dwindle, Scrolling, swap-size preservation, wheel speed and direction, per-output scrolling axis, workspace enable/count/switch direction, minimize placement, maximized padding | Monitor already sets window layout through the shared document. Port the remaining controls. |
| Desktop chrome | System bar placement and thickness, clipboard tray edge and size | Port from Layout. |
| Desktop overlays | Launcher, Dashboard, Notifications panel and HUD placement, anchors, dimensions, margins and hover triggers where offered | Port from Overlays. Do not duplicate notification policy here. |
| Desktop window appearance | Focused window border and color, focused/unfocused window opacity, desktop cursor theme and size, client cursor surfaces | Port the desktop options. Review scope of cursor preferences if mobile pointer use expands. |
| Desktop motion | Window close effect and desktop panel travel | Port from Animations. Keep global animation speed and lock-screen motion with shared controls. |

## Mobile only

| Area | Controls | Current GPUI coverage |
| --- | --- | --- |
| Cellular and SIM | Operator, signal, radio and roaming status, VoLTE, mobile data, active SIM slot, SIM PIN, diagnostics, reconnect and radio/modem restarts | Live via dmail. Gate on service availability, including when desktop mode is active. Roaming policy, network type selection, APN editing and airplane mode are absent. |
| Phone sound and haptics | Ringtone, vibrate for calls, keyboard vibration, touch sounds, ring/alarm volume | Keyboard vibration is live. Ringtone, call vibration and touch sounds are disabled. Ring and alarm currently mirror media volume. |
| Mobile notifications | Wake screen for notifications | Live. Keep Do not disturb and lock-screen notification visibility in Shared. |
| Mobile lock and touch | Lock delay after screen-off, tablet lock-screen layout editor, always-on clock, software keyboard height/autocorrect/suggestions, touch-and-hold delay | Delay and shell editor are live. Always-on clock and software keyboard controls are disabled. Touch-and-hold delay is a hidden placeholder. |
| Phone identity and defaults | SIM phone number, default SMS/phone apps | SIM number is live. Defaults are disabled. |

## Shared

| Area | Controls | Current coverage and migration work |
| --- | --- | --- |
| Displays | Internal brightness, refresh rate, scale, resolution, rotation, variable refresh rate, output enable, primary/automatic output and multi-output arrangement | GPUI has brightness and panel refresh plus scale and refresh for one external monitor. Replace the separate Display/Monitor hardware surfaces with a shared output model. Show advanced controls where supported and arrangement when multiple outputs are connected. Keep per-output desktop layout controls and wallpaper/Droidloom targets reachable from the selected display. |
| Appearance and wallpaper | Light/dark/system preference, accent, font, application theming, material/transparency, glass and blur tuning, panel/card opacity, roundness and per-display wallpaper/placement | GPUI has many of these, including a local and Wallhaven picker and per-monitor wallpaper. Audit enum values and settings scope before merging controls. Flutter also delegates a desktop wallpaper workflow to the shell. |
| Lock screen and security | PIN/password, fingerprint unlock and enrollment, lock wallpaper, system wallpaper preference, blur/dim, clock size, system status and lock-screen motion | Reuse GPUI PIN/enrollment and wallpaper controls. Audit missing Flutter controls and preserve existing authentication and reader capability checks. |
| Notifications and permissions | Do not disturb, lock-screen notification visibility, per-app keyboard-shortcut permission | GPUI has the first two. Port Flutter's per-app Ask/Allow/Deny path. Camera, microphone and location switches are disabled GPUI catalog entries. Recently sent is also unavailable. |
| Audio | Master output volume and per-app playback levels | GPUI has master/media. Add stream discovery and levels using the existing Denial audio stream methods. |
| Connections | Wi-Fi on/off, scan/join/disconnect, passwords, saved and hidden networks, metering and address policy, Bluetooth power, discovery, pairing, connect/disconnect and forget | Reuse GPUI's existing NetworkManager/iwd and BlueZ backends. Some options depend on the manager. Hotspot is disabled, and enterprise Wi-Fi is absent. |
| Power and battery | Idle display off, lock and suspend, associated timers, suspend mode, short/long power-button actions, battery status and health | GPUI has display-off, battery and its mobile lock delay. Port Flutter power controls. Preserve the distinction between elapsed idle time and time since screen-off. |
| Language, date and time | Locale, application language, 24-hour clock, NTP, manual date/time, time zone and Wi-Fi regulatory country | GPUI already has service-backed controls. Reconcile Flutter's locale choices and scope with GPUI's system locale and application environment updates. |
| Physical input and shortcuts | XKB layouts/options, layout-switch binding, repeat delay/rate, pointer speed/acceleration, tap-to-click, touchpad speed/natural scrolling, desktop scrolling-layout swipe speed, shortcut list/edit/validate/restore, Input Deck discovery | Port from Flutter using the keyboard/input/shortcut APIs. Gate on actual input devices. Place the scrolling-layout swipe option under desktop layout or show it conditionally in Touchpad. |
| User and device | User display name/photo, device hostname, model, hardware, OS/kernel/build, uptime and storage | GPUI has device identity and storage. Port Flutter's You page through its account backend. Keep account display name separate from device hostname. |
| Apps and Android | Installed apps, Droidloom enable/restart/status, Android density globally and per output, app open/stop, info/permissions, clear cache/data and uninstall | GPUI already implements these when Droidloom is installed. Default SMS/phone entries remain mobile placeholders. |
| Advanced | Global/per-app launch environment, animation duration scale, developer workspace/setup, live development and auto-reload, hot reload/restart, optimized builds, recovery and diagnostics | Port environment and motion controls. The developer controls are on the GPUI Developer page, listed when the running Denial answers `ui.get` and backed by its `ui.*` operations. |
| Plugins | What the desktop is built from: selection, apply with its running step, the last failure, undo, standard desktop, add from a folder or Git link, update, Denial's defaults, the community list, recent activity | On the GPUI Plugins page, listed where `denial-plugins` is installed. It shows what `denial-plugins status --json` says and replaces the standalone Plugin Manager app, with Denial's defaults, the community list, the plugins included automatically and the recent activity. |
| Accessibility and updates | Bold text, higher contrast, reduced motion, update check/status/channel and Wi-Fi-only downloads | Hidden GPUI catalog placeholders. Exclude from a parity milestone until backed by actual behavior. |

The Flutter About page also contains product credits. Treat these as About content. Flutter’s `--welcome` entry starts a separate Welcome app from the same executable and needs its own packaging and launch decision before removing that executable.

## Adaptive layout and discovery

- Keep one navigation model, search index and settings backend. Use the existing Islands push navigation in compact windows. In roomy desktop windows use a persistent searchable sidebar and independently scrolling detail pane, with keyboard focus, visible selection, and dialogs centered over the detail content.
- Use available logical window width for the layout breakpoint and shell presentation for defaults and ordering. Devices can run mobile and desktop scenes simultaneously, so a startup flag or attached-monitor count cannot identify the scene displaying a particular Settings window. Flutter's presentation provider is in-process. This pass did not establish a live per-window presentation signal exposed to GPUI. Use width as a working fallback and keep both eligible categories discoverable through navigation and search.
- Keep navigation, drafts and dialogs across resize or presentation changes. Review current mobile status-strip/gesture insets and bottom dialog placement when rendering the wide shell. Keep touch input usable in the wide layout.
- Preserve GPUI’s single-instance activation, existing page IDs and shell deep links. Desktop currently invokes `denial-settings --page=…`, while GPUI accepts `settings <page>` and handles activation through `org.denialwm.Settings`. Map routes and activation before changing launchers, including opening Settings from an external desktop scene while an instance already exists.

## GPUI gaps and display arrangement

**No new GPUI drawing primitive is apparent.** The local fork has `canvas`, custom paint and hitboxes, absolute positioning, mouse/drag events, touch-drag events, wheel input, focus/key actions and accessibility metadata. GPUI Settings already claims touches in a canvas-backed slider so the surrounding page does not scroll. Display tiles can be positioned GPUI elements with labels, focus and accessibility, with a canvas for the background or guides as needed.

The missing component is the arrangement editor and its state. Use Flutter’s Displays page and its arrangement geometry as behavioral references. Fit the enabled outputs into a bounded viewport in compositor logical coordinates, accounting for mode, scale and transform. Support selection, edge snapping, dragging, background pan and zoom/fit. Keep drag updates in a draft. Add a keyboard/numeric position path and an output list for selection when tiles are small. Check pointer capture leaving the tile, touch ownership and cancellation inside scrollable content, fractional scaling, keyboard focus/semantics, unplug during a draft, and moving the window to another output. These are verification targets from the static review.

Expand `src/denial.rs` from the first enabled external monitor to the full authoritative output snapshot. Preserve other output fields when editing, including shell-specific fields such as scrolling axis, and check protocol handling of primary selection against the Flutter bridge. Submit the draft as one serial-checked `outputs.apply` transaction using reported capabilities and compositor-owned timed confirmation. Reload on stale configuration or hotplug, and retain explicit Keep/Revert and automatic rollback. Review behavior when the Settings window's own output disappears during confirmation.

Most other pages can use the current rows, sliders and dialogs. The clear app-level additions are a reusable list/detail shell and desktop popovers, a shortcut recorder with compositor validation and conflict feedback, a custom color editor and preview, and account photo selection with Wayland file-picker integration. GPUI has the primitives and a path prompt API. The runtime behavior of these integrations remains to be verified.

## Migration order

1. Add adaptive navigation, capability visibility and route compatibility. Reuse real GPUI pages for both layouts.
2. Port settings-document-backed controls and the existing keyboard/input/shortcuts and audio methods. Preserve unrelated settings and handle revision conflicts. Trace the account and permission service clients separately.
3. Build the multi-output editor against the existing timed transaction APIs. Compare Flutter behavior for snapping, primary selection, rollback and hotplug before replacing the Monitor page.
4. Port remaining advanced workflows and audit launchers, packages and Welcome before retiring Flutter Settings.

## Source map

- [GPUI Settings README](README.md), [catalog and binding kinds](src/catalog.rs), [control client](src/denial.rs), [mobile renderer](src/settings.rs) and [pointer/touch slider](src/settings/page.rs)
- [Flutter Settings entry point](../../../denial/settings_app/lib/main.dart), [page selection](../../../denial/plugins/denial_desktop/lib/src/settings/widgets/settings_navigation.dart) and [page assembly](../../../denial/plugins/denial_desktop/lib/src/settings/settings_application.dart)
- [Flutter Displays editor](../../../denial/plugins/denial_desktop/lib/src/settings/widgets/settings_displays_page.dart), [draft and transaction state](../../../denial/packages/denial_flutter_sdk/lib/src/state/output_configuration.dart) and [snap geometry](../../../denial/packages/denial_flutter_sdk/lib/src/settings/monitor_arrangement.dart)
- [Denial control protocol](../../../denial/docs/protocol/control-v1.md), [shell presentation](../../../denial/plugins/denial_desktop/lib/src/state/reference_shell_presentation.dart), [external desktop scene](../../../denial/plugins/denial_desktop/lib/src/state/phone_desk.dart) and [Welcome app](../../../denial/docs/WELCOME_APP.md)
- [GPUI Canvas](../../../gpui-touch/crates/gpui/src/elements/canvas.rs), [element interactions](../../../gpui-touch/crates/gpui/src/elements/div.rs) and [touch gestures](../../../gpui-touch/crates/gpui/src/gestures.rs)
