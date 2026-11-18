# Flutter Settings migration checklist

This tracks the features in the standalone Flutter Settings app. The scope of each feature is described in [the migration inventory](SETTINGS_MIGRATION.md). Keep **Added** checked when the GPUI app has a working equivalent. Partial coverage is noted inline. Check **Tested** after validating the equivalent in a running Denial session. Compilation and launch alone do not count.

The inventory follows the [Flutter page registry](../../../denial/plugins/denial_desktop/lib/src/settings/widgets/settings_navigation.dart), [page assembly](../../../denial/plugins/denial_desktop/lib/src/settings/settings_application.dart), and [page widgets](../../../denial/plugins/denial_desktop/lib/src/settings/widgets). Welcome is tracked separately in the migration inventory because it runs as its own app.

## You

- Read account name
  - [x] Added
  - [ ] Tested
- Edit and save display name
  - [x] Added
  - [ ] Tested
- Choose and preview a profile photo
  - [x] Added
  - [ ] Tested
- Restore the default profile photo
  - [x] Added
  - [ ] Tested

## Fingerprint

- List enrolled fingerprints
  - [x] Added
  - [ ] Tested
- PIN-gated access and enrollment progress
  - [x] Added
  - [ ] Tested
- Enroll a picked finger by hand and digit (GPUI offers enrollment and naming)
  - [x] Added
  - [ ] Tested
- Cancel enrollment
  - [x] Added
  - [ ] Tested

## Permissions

- List applications with keyboard-shortcut inhibition permissions
  - [x] Added
  - [ ] Tested
- Set Ask, Allow, or Deny for each app
  - [x] Added
  - [ ] Tested

## About

- Denial project wordmark (the tagline is left out by decision)
  - [x] Added
  - [ ] Tested
- Project description and architecture: left out by decision
- Project credits, with what Denial is built on
  - [x] Added
  - [ ] Tested
- Support links (not in the Flutter Settings): sponsor page, GitHub
  Sponsors, Ko-fi, the repository and its issues
  - [x] Added
  - [ ] Tested

## Appearance

- Dark, light, or no preference
  - [x] Added
  - [ ] Tested
- Open a wallpaper picker
  - [x] Added
  - [ ] Tested
- Use wallpaper colors for the shell accent
  - [x] Added
  - [ ] Tested
- Choose an arbitrary custom accent in the color editor (GPUI has presets)
  - [x] Added
  - [ ] Tested
- Reset the custom accent color
  - [x] Added
  - [ ] Tested
- Font family picker and system-default font
  - [x] Added
  - [ ] Tested
- Application theming toggle
  - [x] Added
  - [ ] Tested
- Reapply application theming
  - [x] Added
  - [ ] Tested
- Corner roundness
  - [x] Added
  - [ ] Tested
- Transparency mode (Glass, Blur, Off)
  - [x] Added
  - [ ] Tested
- Blur quality
  - [x] Added
  - [ ] Tested
- Blur opacity threshold
  - [x] Added
  - [ ] Tested
- Panel opacity
  - [x] Added
  - [ ] Tested
- Card opacity
  - [x] Added
  - [ ] Tested
- Glass shell opacity
  - [x] Added
  - [ ] Tested
- Glass app-panel opacity
  - [x] Added
  - [ ] Tested
- Glass window opacity
  - [x] Added
  - [ ] Tested
- Glass frost
  - [x] Added
  - [ ] Tested
- Glass refraction
  - [x] Added
  - [ ] Tested
- Glass refraction depth
  - [x] Added
  - [ ] Tested
- Glass thickness
  - [x] Added
  - [ ] Tested
- Glass dispersion
  - [x] Added
  - [ ] Tested
- Glass bevel width
  - [x] Added
  - [ ] Tested
- Glass edge strength
  - [x] Added
  - [ ] Tested
- Glass rim width
  - [x] Added
  - [ ] Tested
- Glass rim falloff
  - [x] Added
  - [ ] Tested
- Glass lighting angle
  - [x] Added
  - [ ] Tested
- Glass light intensity
  - [x] Added
  - [ ] Tested
- Glass opposite light
  - [x] Added
  - [ ] Tested
- Glass accent tint
  - [x] Added
  - [ ] Tested
- Glass brightness
  - [x] Added
  - [ ] Tested
- Glass saturation
  - [x] Added
  - [ ] Tested
- Glass rendering quality
  - [x] Added
  - [ ] Tested
- Reset glass effects
  - [x] Added
  - [ ] Tested
- Focused-window border toggle
  - [x] Added
  - [ ] Tested
- Focused-window border uses the custom accent colour
  - [x] Added
  - [ ] Tested
- Focused-window opacity
  - [x] Added
  - [ ] Tested
- Unfocused-window opacity
  - [x] Added
  - [ ] Tested
- Choose cursor theme
  - [x] Added
  - [ ] Tested
- Cursor size
  - [x] Added
  - [ ] Tested
- Allow application-provided cursor surfaces
  - [x] Added
  - [ ] Tested
- Import a cursor-theme ZIP
  - [x] Added
  - [ ] Tested
- Remove an imported cursor theme
  - [x] Added
  - [ ] Tested
- Reset appearance
  - [x] Added
  - [ ] Tested

## Language

- Interface locale including system default (GPUI has explicit English and Simplified Chinese)
  - [x] Added
  - [ ] Tested
- Reset language preference
  - [x] Added
  - [ ] Tested

## Keyboard

- Edit XKB layouts and variants
  - [x] Added
  - [x] Tested
- Edit XKB options
  - [x] Added
  - [x] Tested
- Show active keyboard layout
  - [x] Added
  - [x] Tested
- Reset keyboard preferences
  - [x] Added
  - [x] Tested
- Apply and validate keyboard configuration
  - [x] Added
  - [x] Tested
- Show the layout-switch shortcut
  - [x] Added
  - [x] Tested
- Key repeat delay
  - [x] Added
  - [x] Tested
- Key repeat rate
  - [x] Added
  - [x] Tested

## Touchpad and pointer

- Mouse pointer speed
  - [x] Added
  - [x] Tested
- Mouse acceleration
  - [x] Added
  - [x] Tested
- Tap to click
  - [x] Added
  - [ ] Tested
- Touchpad scroll speed
  - [x] Added
  - [ ] Tested
- Natural scrolling
  - [x] Added
  - [ ] Tested
- Scrolling-layout swipe speed
  - [x] Added
  - [ ] Tested

## Shortcuts

- List configured shortcuts
  - [x] Added
  - [ ] Tested
- Add and edit a shortcut chord
  - [x] Added
  - [ ] Tested
- Choose a built-in or plugin action, with arguments when available
  - [x] Added
  - [ ] Tested
- Choose an installed application target
  - [x] Added
  - [ ] Tested
- Configure executable and argument targets
  - [x] Added
  - [ ] Tested
- Configure shell command targets
  - [x] Added
  - [ ] Tested
- Validate shortcut input and report conflicts
  - [x] Added
  - [ ] Tested
- Delete a binding
  - [x] Added
  - [ ] Tested

## Application environment

- View and search application scopes
  - [x] Added
  - [ ] Tested
- View inherited values and overrides
  - [x] Added
  - [ ] Tested
- Add or edit an environment value globally or per app
  - [x] Added
  - [ ] Tested
- Hide an inherited variable
  - [x] Added
  - [ ] Tested
- Remove an override
  - [x] Added
  - [ ] Tested
- Clear one scope
  - [x] Added
  - [ ] Tested
- Reset all environment overrides
  - [x] Added
  - [ ] Tested

## Animations

- Global animation speed (shown as animation duration in GPUI)
  - [x] Added
  - [ ] Tested
- Desktop panel travel
  - [x] Added
  - [x] Tested
- Window close effect (None, Fade, Implode, Explosion)
  - [x] Added
  - [x] Tested
- Lock-screen animations
  - [x] Added
  - [ ] Tested
- Reset animation preferences
  - [x] Added
  - [x] Tested

## Desktop layout

- Window layout (Stacking, Dwindle, Scrolling)
  - [x] Added
  - [x] Tested
- Preserve sizes on scrolling-layout swap
  - [x] Added
  - [x] Tested
- Scrolling-layout wheel speed
  - [x] Added
  - [x] Tested
- Scrolling-layout wheel direction
  - [x] Added
  - [x] Tested
- Enable or disable workspaces
  - [x] Added
  - [x] Tested
- Workspace count
  - [x] Added
  - [x] Tested
- Workspace switching orientation
  - [x] Added
  - [x] Tested
- System bar display selection and cloning
  - [x] Added
  - [x] Tested
- System bar edge
  - [x] Added
  - [x] Tested
- System bar thickness
  - [x] Added
  - [x] Tested
- Minimized window placement
  - [x] Added
  - [x] Tested
- Maximized-window outer padding
  - [x] Added
  - [x] Tested
- Clipboard tray edge
  - [x] Added
  - [x] Tested
- Clipboard tray size
  - [x] Added
  - [x] Tested
- Reset desktop layout
  - [x] Added
  - [x] Tested

## Overlays

- Launcher anchor, width, height, and edge margin
  - [x] Added
  - [x] Tested
- Dashboard anchor, width, height, and edge margin
  - [x] Added
  - [x] Tested
- Notifications panel anchor, width, and edge margin
  - [x] Added
  - [x] Tested
- System HUD anchor, width, and edge margin
  - [x] Added
  - [x] Tested
- Hover triggers where supported
  - [x] Added
  - [x] Tested
- Reset overlay placement
  - [x] Added
  - [x] Tested

## Lock screen

- Live lock-screen preview
  - [x] Added
  - [ ] Tested
- Set, change, or remove the lock PIN
  - [x] Added
  - [ ] Tested
- Choose an independent lock-screen wallpaper or follow the desktop wallpaper (replaces the old toggle)
  - [x] Added
  - [ ] Tested
- Backdrop dimming
  - [x] Added
  - [ ] Tested
- Backdrop blur
  - [x] Added
  - [ ] Tested
- Clock size
  - [x] Added
  - [ ] Tested
- Show system status
  - [x] Added
  - [ ] Tested
- Reset lock-screen preferences
  - [x] Added
  - [ ] Tested

## Audio

- Master output volume
  - [x] Added
  - [ ] Tested
- Per-application playback stream levels
  - [x] Added
  - [ ] Tested
- Refresh audio state
  - [x] Added
  - [ ] Tested

## Displays

- Refresh detected outputs and capabilities
  - [x] Added
  - [x] Tested
- Select an output and inspect its status
  - [x] Added
  - [x] Tested
- Arrange outputs by dragging with edge snapping
  - [x] Added
  - [x] Tested
- Pan the arrangement and zoom in, out, or to fit
  - [x] Added
  - [x] Tested
- Set the primary output or use automatic selection
  - [x] Added
  - [x] Tested
- Enable or disable an output
  - [x] Added
  - [x] Tested
- Select resolution
  - [x] Added
  - [x] Tested
- Refresh rate for any selected output
  - [x] Added
  - [x] Tested
- Rotate an output
  - [x] Added
  - [x] Tested
- Scale presets and custom scale per output
  - [x] Added
  - [x] Tested
- Set the per-output scrolling-layout axis
  - [x] Added
  - [x] Tested
- Hardware brightness per output
  - [x] Added
  - [x] Tested
- Software dimming per supported output
  - [x] Added
  - [x] Tested
- Input Deck availability and waiting status
  - [x] Added
  - [ ] Tested
- Enable the Input Deck
  - [x] Added
  - [ ] Tested
- Choose the Input Deck touch display
  - [x] Added
  - [ ] Tested
- Input Deck keyboard height
  - [x] Added
  - [ ] Tested
- Input Deck touchpad width
  - [x] Added
  - [ ] Tested
- Input Deck touchpad height
  - [x] Added
  - [ ] Tested
- Input Deck pointer speed
  - [x] Added
  - [ ] Tested
- Input Deck natural scrolling
  - [x] Added
  - [ ] Tested
- Input Deck CPU chart
  - [x] Added
  - [ ] Tested
- Input Deck GPU charts
  - [x] Added
  - [ ] Tested
- Toggle adaptive sync where supported
  - [x] Added
  - [x] Tested
- Apply or discard a complete output draft
  - [x] Added
  - [x] Tested
- Keep or revert a timed display configuration
  - [x] Added
  - [x] Tested

## Network

- Wi-Fi power
  - [x] Added
  - [ ] Tested
- Scan and list networks with signal status
  - [x] Added
  - [ ] Tested
- Connect to a network and enter its password
  - [x] Added
  - [ ] Tested
- Disconnect from a network
  - [x] Added
  - [ ] Tested
- Connectivity state including limited access and captive portal
  - [x] Added
  - [ ] Tested

## Bluetooth

- Bluetooth power
  - [x] Added
  - [ ] Tested
- Scan and list paired and nearby devices
  - [x] Added
  - [ ] Tested
- Pair and connect a device
  - [x] Added
  - [ ] Tested
- Disconnect a device
  - [x] Added
  - [ ] Tested

## Power

- Enable or disable idle display power-off
  - [x] Added
  - [x] Tested
- Idle display-off timeout
  - [x] Added
  - [x] Tested
- Enable or disable automatic idle lock
  - [x] Added
  - [x] Tested
- Idle lock timeout
  - [x] Added
  - [x] Tested
- Enable or disable automatic suspend
  - [x] Added
  - [x] Tested
- Idle suspend timeout
  - [x] Added
  - [x] Tested
- Suspend mode
  - [x] Added
  - [x] Tested
- Short power-button action
  - [x] Added
  - [x] Tested
- Long power-button action
  - [x] Added
  - [x] Tested
- Show the notice that applications can pause automatic idle actions
  - [x] Added
  - [ ] Tested
- Battery status and time estimate (existing Battery page)
  - [x] Added
  - [ ] Tested
- Battery charge-threshold control where supported
  - [x] Added
  - [ ] Tested
- Reset power preferences
  - [x] Added
  - [x] Tested

## Developer

- Show runtime mode, status, operation, generation, and errors (the operation shows as the status sentence and its progress)
  - [x] Added
  - [x] Tested
- Set and validate the development workspace (the folder is typed)
  - [x] Added
  - [x] Tested
- Run development setup
  - [x] Added
  - [x] Tested
- Enable or disable live development
  - [x] Added
  - [x] Tested
- Enable or disable automatic reload
  - [x] Added
  - [x] Tested
- Hot reload
  - [x] Added
  - [x] Tested
- Hot restart
  - [x] Added
  - [x] Tested
- Refresh runtime status (GPUI reads it every second while the page shows, so it has no button)
  - [x] Added
  - [x] Tested
- Build optimized shell
  - [x] Added
  - [x] Tested
- Revert to the last working shell
  - [x] Added
  - [x] Tested
- Restore official shell
  - [x] Added
  - [x] Tested
- Show VM service endpoint and attach instructions
  - [x] Added
  - [x] Tested
- Show diagnostics
  - [x] Added
  - [x] Tested

## Plugins

These come from the standalone Plugin Manager app, not from Flutter Settings. The GPUI page shows what `denial-plugins status --json` and `list --json` say ([Denial's plugin manager, section 23](../../../denial/docs/PLUGIN_MANAGER.md)).

- Show what the desktop runs, why, and what to do next
  - [x] Added
  - [x] Tested
- Turn a known plugin on or off
  - [x] Added
  - [x] Tested
- Show what stops the selection from being applied
  - [x] Added
  - [x] Tested
- Apply, with the running step
  - [x] Added
  - [x] Tested
- Switch now while a rebuild waits for a pause
  - [x] Added
  - [x] Tested
- Show the last failure, its message and its log
  - [x] Added
  - [x] Tested
- Undo the last change
  - [x] Added
  - [x] Tested
- Return to the standard desktop
  - [x] Added
  - [x] Tested
- Add a plugin from a folder or a Git link
  - [x] Added
  - [x] Tested
- Update plugins
  - [x] Added
  - [x] Tested
- Prepare plugin support
  - [x] Added
  - [x] Tested
- Use Denial defaults
  - [x] Added
  - [ ] Tested
- Browse and refresh the community catalog
  - [x] Added
  - [ ] Tested
- List the plugins included automatically
  - [x] Added
  - [ ] Tested
- Activity history
  - [x] Added
  - [ ] Tested
