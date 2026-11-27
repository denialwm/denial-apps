# Polkit agent

Denial's Polkit authentication agent and its dialogs, one GPUI program, on
the desktop and the phones. Denial builds it with Settings and installs it
with the `denial` package as `/usr/lib/denial-apps/polkit`, with the command
`denial-polkit-agent` linked to it; see Denial's `settings/README.md`.

| Started as | What it is |
|---|---|
| `denial-polkit-agent` | The agent: registers with polkitd for the login session and opens the dialog only while a request is in progress |
| `denial-polkit-agent --prompt` | The authentication dialog: the password or PIN prompt. The agent starts it |
| `denial-polkit-agent --access` | denial-portal's Access request: whether an app may have the camera, the microphone, the location or all keyboard shortcuts. The portal starts it |

The agent starts the dialog as `/proc/self/exe`: when a package update
replaces the file, the dialog still is the agent's own program, and the two
always speak the same protocol.

## The dialogs

Each dialog is a centered, undecorated wlr-layer-shell overlay with exclusive
keyboard focus, named `dev.denial.Polkit` or `dev.denial.Access` so Denial's
shell treats it as its own.

- **Desktop.** A 600 × 520 transparent canvas with the card in its middle,
  520 wide. The compositor frosts the desktop under the overlay wherever its
  alpha passes the user's backdrop-blur opacity threshold: the card's glass
  tint stays above the threshold (at least the app-panel opacity, 0.34 and
  threshold + 0.05) and its shadow below, so the card is glass and its
  shadow is not. With transparency off the card is opaque raised, as
  `APPLICATION_MATERIALS.md` has dialogs.
- **Phone** (`DENIAL_SHELL_PROFILE=mobile`). The overlay is the card: it
  spans the screen less an 8 lp gutter each side and takes the card's
  height once laid out. The shell centers it between the status bar and the
  keyboard, dims the rest and draws the phone's window glass under all of
  it, rounded with its panel radius, which the card takes too. The card is
  an island of `MOBILE_APP_MATERIALS.md`, at least the island floor, as
  Settings' dialogs are; its buttons share the row.

The look comes from Denial's settings and the wallpaper accent through
`denial-look`, read once when the dialog opens. Escape and the Back gesture
cancel, as Cancel does.

The authentication card shows the account that will authorize (Denial's
own profile name and picture when it is the user themselves, else the GECOS
name and the AccountsService or `~/.face` picture, else a monogram), what is
asked, the command `pkexec` will run when Polkit's caller says, and the reply
field. A password field shows dots and never copies its text out; Caps Lock
shows beside it. Several accounts that may authorize are offered as a list
first. Polkit's and PAM's messages show under the field, a time limit or a
lockout as a badge beside the buttons. Confirm says "Preparing…" or
"Verifying…" while it waits.

The Access card shows the app's icon and name from its desktop entry, the
caller's title, subtitle, body and choices (a switch for a yes-or-no
choice, buttons for the others), and the answers. When the caller offers to
remember the answer, the answers are Always allow, Allow once (the colored
one), Never allow and Decline once. Enter never grants.

## Authentication and lifecycle

- Only the unique system-bus owner of the Polkit authority can begin or
  cancel requests; the owner is pinned for each registration.
- Registration uses the current Unix session. A user-manager service is
  outside session cgroups, so it finds the compositor's session through the
  Wayland socket's peer credentials. Another agent already registered for
  the session keeps it: this one logs a warning and exits successfully.
- `PolkitAgentSession` owns the conversation and runs Polkit's privileged
  helper. This program implements no PAM and never reports success itself;
  nothing the dialog sends can grant anything.
- Requests are serialized, with up to eight queued, three failed attempts
  each and five minutes for an open dialog. A dialog's late replies are
  ignored. A cancellation that arrives before its request still stops it.
- Locking or deactivating the session cancels active and queued requests. A
  closed dialog or an expired deadline cancels the request.
- When polkitd goes away the requests are cancelled before registering
  again; registration is retried a few times, then the systemd unit restarts
  the agent.
- Replies travel through a private socket, never argv, the environment,
  files or logs. The buffers this program writes them through are cleared
  after use; the text field's own copies are not guaranteed to be.

## Turning it off

- `deniald --no-polkit-agent` publishes `DENIAL_POLKIT_AGENT=0` to the
  session; the agent then exits at once.
- `DENIAL_POLKIT_AGENT=0` in `/etc/denial/session.conf` makes
  `denial-session` pass that flag; the agent honours the variable directly.
- On NixOS, `programs.denial.polkitAgent.enable = false`.

## Dialog sockets, schema 1

The starter makes a socket in a random folder of mode 0700 under
`$XDG_RUNTIME_DIR`, the socket mode 0600, and starts the dialog with the
socket's path and a random token in the environment. Every message is UTF-8
JSON on one line, at most 16 KiB. The dialog connects and says the token
within 30 seconds. When the starter closes the socket the dialog exits.

Authentication (`DENIAL_POLKIT_SOCKET`, `DENIAL_POLKIT_TOKEN`); the cookie
never reaches the dialog:

| Direction | Type | Fields |
| --- | --- | --- |
| dialog → agent | `hello` | `token` |
| agent → dialog | `request` | `schema`, `action`, `message`, `icon`, `command`, `runAs`, `identities: [{uid, name}]` |
| dialog → agent | `select` | `uid` from the identities; once, before authentication |
| agent → dialog | `prompt` | `prompt` (increasing id), `text`, `echo` |
| dialog → agent | `response` | the matching `prompt`, `text` |
| agent → dialog | `info` / `error` | `text` |
| agent → dialog | `failed` | `text`: the attempts are used up; the agent closes the dialog shortly |
| dialog → agent | `cancel` | none |

Access (`DENIAL_ACCESS_SOCKET`, `DENIAL_ACCESS_TOKEN`), served by
denial-portal's `compositor/denial-portal/src/access.rs`:

| Direction | Type | Fields |
| --- | --- | --- |
| dialog → portal | `hello` | `token` |
| portal → dialog | `request` | `schema`, `appId`, `title`, `subtitle`, `body`, `grantLabel`, `denyLabel`, `choices: [{id, label, options: [{id, label}], initial}]` |
| dialog → portal | `answer` | `grant`, `choices: {id: value}` |
| dialog → portal | `cancel` | none |

A choice without options is a boolean, `"true"` or `"false"`.

The process boundary isolates faults; it is no sandbox against other
programs of the same user.

## Building

It is a member of Settings' Cargo workspace (`apps/settings/Cargo.toml`),
with its lock and profile: Denial builds both in one Cargo run. From
`apps/settings`, `cargo build --release -p polkit`. It links Polkit's
`polkit-agent-1` and `polkit-gobject-1` and GLib's `gobject-2.0`.
