# Calls

Phone and contacts app for Denial phones, written with GPUI. Calls are
mocked for now: dmail's `org.dmail.Voice1` answers NotSupported to
everything, so nothing is dialled. Placing a call rings on screen, the other
side answers after 2.5 s, and the call runs until End. The call screen's top
group says "Mock call". The menu can simulate a call coming in from a
favourite or from a number no one has; it rings for 30 s, then is missed.

It is drawn to Denial's mobile materials, Islands
(denial/docs/MOBILE_APP_MATERIALS.md), as Messages is, and shares its
structure: one screen stack with the same motion, the keyboard inset, and
one Back action for Denial's Back gesture (`back`) and Escape. Content
scrolls under the bars, the gesture zone and the keyboard, never faded; no
bar moves for the keyboard, which only rises over them. A tab bar anchored
at the bottom, opaque and the window's full width, switches between three
tabs, which slide side by side:

- **Recents**: the call log in day islands (Today, Yesterday, Earlier). A
  row shows who, what happened and when, and calls back from its button.
  Tapping a row opens whom it was with; a long press lifts it with Call,
  Message, Copy and Delete, which asks first.
- **Contacts**: a favourites island, then one island per initial letter with
  the letter inside it. A long press lifts a contact with Call, Message and
  Favourite.
- **Keypad**: from the bottom, the round green Call with backspace beside it
  (a long press clears the number), the keys, each an island in a run with
  its letters under the digit (a long press on 0 types +), and the number,
  large, on its own island, with the contact it belongs to under it. Above
  them up to three suggestions: contacts whose number holds the digits or
  whose name they spell on the keys' letters (4-4-8 finds Giulia), and
  recent numbers no one has. A tap puts a suggestion's number on the keypad;
  its button calls it. Call on an empty keypad brings back the number called
  last. A hardware keyboard types on it too.

Recents and Contacts have a search bar at the top, with the round menu
button beside it; the keypad has only the button. Both leave as the list
scrolls on and come back as soon as it scrolls back. The search bar finds
contacts by name or number; a typed number no one has is an accent island
that calls it.

A contact's detail has the avatar over the name island, Call, Message and
Share in one glass group (Share copies the name and numbers), the numbers
with their own Message and Call (a long press copies one), and the recent
calls. The star in the top group makes the contact a favourite. A number no
one has gets the same screen with Copy in place of Share.

The call screen has whom the call is with at the top, then Mute, Keypad,
Speaker, Hold, Message and Details in one glass group over End. While a call
rings here, Decline and Answer take their place. The in-call keypad sends
what it types as tones, shown under the name. Back leaves the call running:
other screens show it as a bar, over the tab bar on the tabs, with End, and a
tap on the bar goes back to it. One call at a time.

Contacts come from the contacts folder, `~/.local/share/contacts`, through
`libraries/denial-contacts`, and follow its changes. Until there are real
ones, 18 sample contacts and a sample call log are shown; the menu hides them
and shows them again. Their numbers are in the ranges Ofcom keeps for
fiction, and the first four are the people of Messages' sample
conversations, on the same numbers. Favourites are kept on the phone, in
`$XDG_STATE_HOME/denial/calls/favourites`; the call log is not kept.

Message opens Messages on the number: `messages <number>`, from beside the
Calls binary, else from the PATH. A running Messages takes it.

`calls <number>` opens the keypad with the number on it. One Calls runs at a
time: it owns `org.denialwm.Calls` on the session bus, and a second `calls
<number>` hands the number and its activation token to it through `Open` and
exits.

## Dependencies

- `libraries/denial-look`, `libraries/denial-look-gpui` and
  `libraries/denial-contacts`, as in Messages.
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
