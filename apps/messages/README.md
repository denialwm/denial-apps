# Messages

SMS app for Denial phones, written with GPUI. Messages live in Phonewave's
modem service, dmail: the app reads and sends them over `org.dmail.Messages1`
on the system bus (`src/dmail.rs`), follows its signals and the modem state in
`org.dmail.Modem1`. The phone's user must be in the `dmail` group.

It is drawn to Denial's mobile materials, Islands
(denial/docs/MOBILE_APP_MATERIALS.md): bubbles, conversations and rows are
opaque islands, navigation sits in glass groups, short labels are chips, and
every text field is in the bottom zone (`Messages::bottom_zone`), the one
place that follows the on-screen keyboard. Messages takes Denial's keyboard
inset, so the shell does not lift the window: the bottom zone moves onto the
keys with the keyboard and the content above it shrinks. The screens: the
conversation list (Pinned, Today, Yesterday, then months; an Unread filter),
search through dmail's `Search`, New conversation, the thread, the archive,
settings (delivery reports, the SIM dmail uses, blocked numbers) and the
folder of blocked messages. A long press lifts a message or a conversation and shows its
actions under it; deleting and blocking ask first. Denial's Back gesture
(`back`) and Escape run one Back action that undoes the innermost state, and
on the list closes the window. The look follows Denial's settings live.

`messages <peer>` opens on the conversation with that number or sender. One
Messages runs at a time: it owns `org.denialwm.Messages` on the session bus
(`src/app.rs`), and a second `messages <peer>` hands the peer and its
activation token to it through `Open` and exits, so the open window shows the
conversation and Denial raises it. Drafts, pinned conversations and those
marked unread are kept on the phone, in `$XDG_STATE_HOME/denial/messages/`
(`src/local.rs`); dmail has no pins and cannot mark a conversation unread.

Names and photos come from the contacts folder, `~/.local/share/contacts`,
through `libraries/denial-contacts`, and follow its changes. Unknown numbers
stay numbers. The composer grows to six lines, then scrolls; Enter starts a
new line, as on phones, and Ctrl+Enter sends from a hardware keyboard. Colour
emoji come from Noto Color Emoji, named first among the font's fallbacks.

What is still missing for a daily SMS app, and what needs dmail:
[PLAN.md](PLAN.md).

`messages-notifier`, the crate's second binary, posts a notification for each
new SMS, one per conversation, as the default SMS app does on Android; dmail
never notifies. It uses zbus only, no GPUI. Notifications carry the contact's
name when the number has one. Tapping one opens its conversation in the
running Messages, or starts Messages on it, with Denial's activation token;
"Mark as read" marks the conversation read in dmail. A conversation read or
deleted anywhere takes its notification away. `packaging/messages-notifier.service`
starts it with the graphical session.

## Dependencies

- [dmail](https://github.com/denialwm/phonewave/tree/main/services/modem/dmail)
  on the phone, for `org.dmail.Messages1`.
- `libraries/denial-look`: Denial's application look (palettes, floors,
  motion and layout tokens, from `~/.config/denial/settings.json`).
- `libraries/denial-look-gpui`: islands, glass groups, chips, the scroll fade
  and the icons, as GPUI elements.
- `libraries/denial-contacts`: the contacts folder, and the number matching
  dmail uses.
- `noto-fonts-emoji` on the phone, for colour emoji.
- [denialwm/gpui-touch](https://github.com/denialwm/gpui-touch), branch
  `denial/touch-fling-end`: zed's GPUI with Wayland touch
  (zed-industries/zed#64079) plus Denial's touch fixes. Pinned by commit;
  push the fork before bumping `rev` here. For now Messages builds against
  the local checkout, `../gpui-touch`, whose uncommitted patches it needs:
  activating a window with another program's token, colour emoji through a
  font's fallbacks (an emoji font may be a fallback though it has no Latin
  letters; a character followed by VS16 takes a fallback that has it), and
  the keyboard inset of Denial's insets protocol version 2 (branch
  `denial/keyboard-inset`).

Denial sends touch only as `wl_touch`, so upstream GPUI cannot be used on
devices.

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
