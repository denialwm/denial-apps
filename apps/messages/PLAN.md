# Messages: plan to a daily SMS app

Written 2026-10-21. Compared against what Android Messages and iOS Messages do
for plain SMS (no RCS, no iMessage).

## Where it starts

The app lists dmail's conversations, opens a thread (its newest 500
messages), sends, follows `MessageReceived` and `MessageChanged`, and filters
the list by peer and last message. The notifier posts one notification per
conversation and opens the app on tap.

dmail's `Messages1` today: `ListConversations`, `ListMessages(peer, before,
limit)` paged by time, `Send`, `MarkRead(peer)`, `Delete(id)`, and the two
signals. No search, no conversation delete, no read signal, no own number, no
delivery reports, slot 1 only.

dmail itself was untracked in Phonewave and edited this morning by the PIN
unlock session, so this round changes the app only. Every dmail change below
is a proposal.

Done on 2026-10-21, on aston: every P1 row below. The app loads the list and
opens threads against the live dmail; sending is untested there because dmail
reported the modem `failed` all session (QMI error 0x004a on the operating
mode, since the 09:59 UTC boot).

Done in round 2, the same day: contacts (`libraries/denial-contacts`, names
and photos everywhere, recipients picked by name or number), the growing
composer, one window over the session bus, and colour emoji on aston. The
app's side of the dmail rows below waits for dmail's API, which a separate
round adds from the section "dmail API" at the end.

Done in round 3, the same day: the Islands look on every screen, and dmail's
new API: archive, block and the blocked folder, delivery states, the settings
screen (delivery reports, SIM slot, blocked numbers), `DeleteConversation`,
`Search`, and the notifier closing a notification on `ConversationRead` and
`ConversationDeleted`. Pins and Mark unread stay in the app (`src/local.rs`).

## Gaps

Size is at our pace. P1 is this round, P2 next, P3 when it is asked for.

| Gap | Where | Size | Priority |
|---|---|---|---|
| New conversation: type a number, or pick an existing conversation | app | ½ day | P1 |
| Older messages when scrolling up (now a fixed 500) | app; dmail pages by second, see note 1 | ½ day | P1 |
| Failed message: tap to retry, delete | app (Send again, Delete the old one) | ¼ day | P1 |
| Long press on a message: copy, delete, details (full time and state) | app | ½ day | P1 |
| Delete a conversation | app (Delete per message); dmail `DeleteConversation` later | ¼ day | P1 |
| Drafts kept per conversation, across restarts | app (file in `$XDG_STATE_HOME`) | ¼ day | P1 |
| Segment counter, GSM 7-bit or UCS-2, as dmail splits | app (copy of dmail's rules) | ¼ day | P1 |
| Links, phone numbers, e-mail and codes tappable; codes copy | app | ½ day | P1 |
| Search inside message text | app reads every thread once per search; dmail `Search` later | ¼ day | P1 |
| States when dmail is down, has no access, or the modem is not registered | app (Modem1.State, name owner) | ¼ day | P1 |
| No reply box for alphanumeric senders (dmail refuses them) | app | small | P1 |
| Notification: open the conversation it is about; "Mark as read" action | app + notifier | ¼ day | P1 |
| Multi-line composer that grows to 6 lines, then scrolls | app (`input.rs`) | done, round 2 | P2 |
| One window: `org.denialwm.Messages`, `Open(peer, token)` | app; gpui-touch `activate_window_with_token` | done, round 2 | P2 |
| Unread state right everywhere: reading in the app leaves the notification up | dmail: signal `ConversationRead(s peer)`; notifier closes it | ¼ day | P2 |
| Delete a conversation in one call, with a signal other clients follow | dmail: `DeleteConversation(s peer)`, signal `ConversationDeleted` | ¼ day | P2 |
| Search in dmail (SQLite `LIKE` or FTS5) instead of reading every thread | dmail: `Search(s text, u limit) → a(stsxs)` | ¼ day | P2 |
| Archive conversations | dmail flag (decision 5), then app | ½ day | P2 |
| Emoji in colour | aston: `noto-fonts-emoji`; gpui-touch: fallback fonts need no 'm'; app names the font | done, round 2 | P2 |
| Contact names and avatars | `libraries/denial-contacts` on the vCard folder | done, round 2 | P2 |
| Contacts app that writes the cards; vdirsyncer sync | new app; Phonewave or user setup | later | P3 |
| Inline reply from the notification | Denial's notification server has no `inline-reply`; shell work first | 1–2 days | P3 |
| Delivery reports, off by default, a setting turns them on | dmail, then app setting | 1 day | P2 |
| Blocking numbers, kept in a hidden blocked folder | dmail, then app | ½ day dmail + ¼ app | P3 |
| Dual SIM: slot 1 by default, set once in settings | dmail, then app setting; per-conversation picker later | 1–2 days | P3 |
| MMS: pictures, group texts | not now (decision 2); see note 2 | ~2 weeks | — |

Note 1: `ListMessages` returns messages with `time < before`. Paging with the
oldest loaded time would skip messages sharing that second, so the app asks
for `time <= oldest` (`before = oldest + 1`) and drops ids it already has.
That holds while fewer than a page of messages share one second. A cursor of
`(time, id)` in dmail would make it exact: `ListMessages` could take the
oldest id instead of a time.

Note 2, MMS honestly: receiving needs WAP push over SMS (8-bit, port
addressing, WSP), the m-notification-ind, an HTTP GET to the carrier's MMSC
over its MMS APN (a second WDS session on aston, or the internet one when
the carrier allows it), m-notifyresp-ind, and storing parts on disk. Sending
needs m-send-req and an HTTP POST. Group texts are MMS with several
recipients, so conversations stop being one peer: dmail's store and every
`Messages1` call that takes a peer change shape. In the app: images in
bubbles, a picker (the file chooser portal), group headers. Roughly a week in
dmail, a few days in the app, plus carrier testing. Not started.

## Decisions (the user, 2026-10-21)

1. Contacts are a folder of vCard files, `~/.local/share/contacts`.
   vdirsyncer syncs it later. A native Contacts app comes later and owns
   writing the cards; Messages and `denial-contacts` only read them. Since
   round 3, vdirsyncer's collection subfolders are read one level deep, and
   a card in more than one counts once, by its UID.
2. MMS: not now. Messages stays SMS only.
3. Delivery reports: off by default, a setting turns them on. Needs dmail.
4. Dual SIM: slot 1 by default, set once in settings. A per-conversation
   picker comes later. Needs dmail.
5. Archive: the flag lives in dmail's store, so every client sees it.
6. Blocking: dmail keeps a blocked sender's messages in a hidden blocked
   folder instead of dropping them.
7. Emoji font: `noto-fonts-emoji` on aston, recorded in Phonewave's aston
   README. Installed 2026-10-21.
8. Inline reply from notifications: later, once Denial's notification server
   has the capability.

## dmail API

What items 3 to 6 need from dmail, as proposed to the dmail round. Once dmail's
DESIGN.md describes them, it is the authority. Names follow `Data1`: a
property, a setter method, state saved in `/var/lib/dmail/`. Signals let every
client follow without polling.

| Item | Interface | Member | Store |
|---|---|---|---|
| 3 | `Messages1` | property `DeliveryReports` `b`, default false; `SetDeliveryReports(b enabled)` | file `delivery-reports` |
| 3 | `Messages1` | states gain `delivered`, after every part's SMS-STATUS-REPORT says so, and `undelivered` on a permanent failure; `MessageChanged` carries them | table `parts (message INTEGER, reference INTEGER, status INTEGER)`: one row per sent part, TP-MR and its report |
| 4 | `Modem1` | property `Slot` `y`, 1-based; `SetSlot(y slot)` reopens the provisioning session on that slot | file `slot`, default from `device.toml` `[modem] slot` |
| 4 | `Messages1` | none yet; the later picker gets `Send(s peer, s text, y slot)` | column `messages.slot INTEGER NOT NULL DEFAULT 1`, the slot each message went or came through |
| 5 | `Messages1` | `ListConversations` rows gain a sixth field, archived: `a(ssxbub)`; `SetArchived(s peer, b archived)`; signal `ConversationArchived(s peer, b archived)`. A received message unarchives its conversation | table `conversations (peer TEXT PRIMARY KEY, archived INTEGER NOT NULL DEFAULT 0)` |
| 6 | `Messages1` | `Block(s number)`, `Unblock(s number)`, `ListBlocked() → as`, `ListBlockedMessages() → a(tssx)` (id, peer, text, time); signal `BlockedChanged()`. Blocked messages are stored read, raise no `MessageReceived`, and stay out of `ListConversations`, `ListMessages` and `Search`. `Unblock` returns them to their conversation | table `blocked (number TEXT PRIMARY KEY)`, matched by the last-9-digits rule; column `messages.blocked INTEGER NOT NULL DEFAULT 0` |

The P2 rows `ConversationRead(s peer)`, `DeleteConversation(s peer)` with
`ConversationDeleted(s peer)`, and `Search(s text, u limit) → a(stsxs)` stand
as written in the table above.
