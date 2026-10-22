//! The Messages window: its state, its line to dmail, what each action does,
//! and how its screens are stacked and animated. The screens are drawn in the
//! modules below, all to Denial's mobile materials (Islands,
//! denial/docs/MOBILE_APP_MATERIALS.md).

mod list;
mod parts;
mod settings;
mod thread;

use denial_look::words::{fill, tr};
use std::cell::Cell;
use std::path::PathBuf;
use std::pin::pin;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ::messages::dmail::{
    BlockedRow, ConversationRow, FoundRow, MessageRow, Messages1Proxy, Modem1Proxy,
};
use denial_contacts::{Contacts, Watch, dialable, same_number};
use denial_look::mobile::{
    COMPOSER_HEIGHT, GROUP_HEIGHT, ScrollAway, bottom_inset, content_bottom, dp as dp_lp,
};
use denial_look::{
    INSET, Look, SettingsWatch, Tween, decelerate, ease_in_cubic, ease_out_cubic, motion, stagger,
};
use denial_look_gpui::{Icon, backing, dp, glass_group, solid};
use futures_lite::{Stream, StreamExt};
use gpui::{
    AnyElement, App, AsyncApp, ClipboardItem, Context, Div, Entity, FocusHandle, Focusable, Font,
    FontFallbacks, KeyboardInset, ListAlignment, ListOffset, ListState, SharedString, Subscription,
    Task, WeakEntity, Window, actions, canvas, div, prelude::*, px,
};

use crate::data::{self, Conversation, Message};
use crate::detect::Kind;
use crate::local::{Drafts, Flag, Peers};
use crate::people::People;
use denial_look_gpui::input::{Submitted, TextInput};

actions!(messages, [Back]);

/// Messages read at a time: the newest when a thread opens, then older ones
/// as it scrolls up.
const PAGE: u32 = 60;
/// Newest messages reread after a send or a received message.
const TAIL: u32 = 20;
/// Older messages load once one of a thread's first this many shows.
const OLDER_MARGIN: usize = 8;
/// Search hits asked of dmail at a time.
const HITS: u32 = 50;
/// Messages of the blocked folder read at a time.
const BLOCKED_PAGE: u32 = 100;
/// Lines the composer grows to before it scrolls.
const COMPOSER_LINES: usize = 6;
/// Items past this many arrive together: further ones are off screen.
const STAGGERED: usize = 12;
/// Emoji the UI font lacks are drawn from this font first. Left to
/// cosmic-text, under GPUI, they come from its fixed fallback order, where
/// DejaVu Sans precedes it and draws the emoticons and hearts in monochrome.
const EMOJI_FONT: &str = "Noto Color Emoji";
const NOT_RUNNING: &str = "The modem service is not running";

#[derive(Clone, PartialEq)]
enum Screen {
    /// The conversations; with text in the search field, what it finds.
    List,
    Archived,
    /// Picking who a new conversation goes to.
    Compose,
    /// The conversation with this peer.
    Thread(String),
    Settings,
    /// Messages from blocked senders.
    Blocked,
}

/// The app's line to dmail, which keeps the messages.
enum Link {
    Connecting,
    Ready(Messages1Proxy<'static>),
    /// Why dmail cannot be reached.
    Unavailable(&'static str),
}

enum Event {
    /// dmail started (true) or went away.
    Owner(bool),
    Modem(String),
    Slot(u8),
    Reports(bool),
    Received(String),
    Changed(u64, String),
    Read(String),
    Deleted(String),
    Archived(String, bool),
    BlockedChanged,
}

/// What a long press lifted.
#[derive(Clone, PartialEq)]
enum Held {
    /// A message in a thread or in the blocked folder.
    Message(u64),
    Conversation(String),
}

/// Something that cannot be undone, asked about first.
#[derive(Clone, PartialEq)]
enum Confirm {
    DeleteMessage(u64),
    DeleteConversation(String),
    Block(String),
}

/// What sits over the screen and dims the rest of it.
#[derive(Clone, PartialEq)]
enum Overlay {
    /// Lifted, with its actions under it.
    Held(Held),
    /// The list's menu, under the top group.
    Menu,
    Confirm(Confirm),
}

/// Which of the screens drawn at once: the one shown, or the one leaving
/// during a switch, which takes no input.
#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Shown,
    Leaving,
}

impl Layer {
    fn live(self) -> bool {
        self == Layer::Shown
    }
}

/// Where an island is drawn while its screen comes or goes: faded and moved
/// down by `offset` lp.
#[derive(Clone, Copy)]
struct Motion {
    opacity: f32,
    offset: f32,
}

impl Motion {
    const STILL: Motion = Motion {
        opacity: 1.0,
        offset: 0.0,
    };
}

/// How the shown screen came in: at launch, opened from another screen, or
/// back from one, which leaves meanwhile.
struct Arrival {
    clock: Tween,
    launch: bool,
    /// Back runs the opening in reverse: the leaving islands drop as they
    /// fade, and the arriving ones only fade in.
    back: bool,
    leaving: Option<Screen>,
}

impl Arrival {
    /// Island `index`, counted from the one nearest where the eye starts.
    fn item(&self, index: usize, now: Instant) -> Motion {
        let index = index.min(STAGGERED);
        let (t, rise) = if self.launch {
            let delay = stagger(motion::LAUNCH_DELAY, motion::LAUNCH_STEP, index);
            let t = self.clock.segment(delay, motion::LAUNCH).progress(now);
            (ease_out_cubic(t), dp_lp(motion::LAUNCH_RISE))
        } else {
            let delay = stagger(motion::OUT, motion::IN_STEP, index);
            let t = self.clock.segment(delay, motion::IN).progress(now);
            (decelerate(t), if self.back { 0.0 } else { motion::IN_RISE })
        };
        Motion {
            opacity: t,
            offset: rise * (1.0 - t),
        }
    }

    /// An island of the leaving screen.
    fn leaving_item(&self, now: Instant) -> Motion {
        let gone = self.gone(now).unwrap_or(1.0);
        Motion {
            opacity: 1.0 - gone,
            offset: if self.back {
                motion::IN_RISE * gone
            } else {
                0.0
            },
        }
    }

    /// The contents of the glass groups.
    fn group(&self, now: Instant) -> f32 {
        if self.launch {
            self.clock.segment(0, motion::GROUP_FADE).progress(now)
        } else {
            let in_ = self.clock.segment(motion::GROUP_IN_DELAY, motion::GROUP_IN);
            in_.progress(now)
        }
    }

    /// The bottom zone: it fades in at launch and rises in on a switch.
    fn bottom(&self, now: Instant) -> Motion {
        if self.launch {
            return Motion {
                opacity: self.group(now),
                offset: 0.0,
            };
        }
        let t = decelerate(
            self.clock
                .segment(motion::BOTTOM_DELAY, motion::IN)
                .progress(now),
        );
        Motion {
            opacity: t,
            offset: motion::BOTTOM_TRAVEL * (1.0 - t),
        }
    }

    /// How far the leaving screen is gone, 0 to 1, while it shows.
    fn gone(&self, now: Instant) -> Option<f32> {
        let gone = self.clock.segment(0, motion::OUT).progress(now);
        (self.leaving.is_some() && gone < 1.0).then(|| ease_in_cubic(gone))
    }

    fn running(&self, now: Instant) -> bool {
        let end = if self.launch {
            stagger(motion::LAUNCH_DELAY, motion::LAUNCH_STEP, STAGGERED) + motion::LAUNCH
        } else {
            stagger(motion::OUT, motion::IN_STEP, STAGGERED) + motion::IN
        };
        self.clock.segment(end, 0).running(now)
    }
}

/// How much of the window the on-screen keyboard covers, in lp, as the shell
/// moves it from `from` to `to` over `motion`.
struct Keyboard {
    from: f32,
    to: f32,
    motion: Tween,
}

impl Keyboard {
    /// The height now, decelerating like the shell's keyboard.
    fn height(&self, now: Instant) -> f32 {
        denial_look::lerp(
            self.from,
            self.to,
            ease_out_cubic(self.motion.progress(now)),
        )
    }
}

/// Someone a new conversation can go to.
struct Candidate {
    /// The number or sender name the conversation goes to.
    peer: String,
    /// The contact's name, else the peer.
    title: String,
    /// The conversation's last message, or which of the contact's numbers.
    detail: String,
    /// A number typed in, which no conversation or contact has yet.
    typed: bool,
}

/// An item of the search results, top to bottom.
enum Found {
    Chip(String),
    /// One of dmail's hits.
    Hit(usize),
    /// A conversation whose name or number matches.
    Conversation(usize),
}

pub struct Messages {
    look: Look,
    /// The user's font, with colour emoji first among its fallbacks.
    font: Font,
    focus_handle: FocusHandle,
    /// Conversations dmail has no message in yet, then dmail's, newest first.
    conversations: Vec<Conversation>,
    link: Link,
    /// dmail's modem state; sending needs `registered`.
    modem: String,
    /// The SIM dmail uses, 1 or 2; 0 until dmail says.
    sim_slot: u8,
    /// Whether dmail asks for delivery reports.
    delivery_reports: bool,
    /// The numbers and sender names dmail blocks.
    blocked: Vec<String>,
    /// The blocked folder, newest first: read for the settings screens.
    blocked_messages: Vec<BlockedRow>,
    /// The contacts, for names and photos.
    people: People,
    search: Entity<TextInput>,
    /// The search text dmail was last asked about.
    query: String,
    /// dmail's hits, newest first, and the text they were found for.
    hits: (String, Vec<FoundRow>),
    /// The number a new conversation goes to.
    recipient: Entity<TextInput>,
    composer: Entity<TextInput>,
    drafts: Drafts,
    pins: Peers,
    /// Conversations marked unread here; dmail cannot mark them.
    marked: Peers,
    /// The made-up conversations of `samples` are shown.
    samples: Flag,
    /// The list's top group holds the search field.
    searching: bool,
    /// The top group leaves as the conversations scroll on, and comes back
    /// as they scroll back.
    top_away: ScrollAway,
    overlay: Option<(Overlay, Tween)>,
    /// A short note above the bottom zone, until its task hides it.
    notice: Option<(SharedString, Tween, Task<()>)>,
    screen: Screen,
    arrival: Arrival,
    /// The on-screen keyboard, which the composer rides on.
    keyboard: Keyboard,
    /// How tall the thread's composer was last drawn, lp, for the thread to
    /// end above it.
    composer_height: Rc<Cell<f32>>,
    rows: ListState,
    archive: ListState,
    found: ListState,
    candidates: ListState,
    /// The open thread, one item per message, kept at the bottom like a chat.
    thread_list: ListState,
    /// What each list's items are, as of the last frame: indices into
    /// `conversations` for `rows` and `archive`.
    shown: Vec<usize>,
    archived_shown: Vec<usize>,
    found_items: Vec<Found>,
    candidate_items: Vec<Candidate>,
    _follow: Task<()>,
    _contacts: Task<()>,
    _settings: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Messages {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Messages {
    /// Opens on the list, or on the conversation with `peer`.
    pub fn new(peer: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let look = Look::load();
        let search = cx.new(|cx| TextInput::new(tr("Search"), &look, 1, cx));
        let recipient = cx.new(|cx| TextInput::new(tr("Name or phone number"), &look, 1, cx));
        let composer = cx.new(|cx| TextInput::new(tr("Message"), &look, COMPOSER_LINES, cx));
        let mut subscriptions = vec![
            cx.observe(&search, |this, _, cx| this.find(cx)),
            cx.observe(&recipient, |this, _, cx| {
                this.candidates.reset(0);
                cx.notify()
            }),
            cx.observe(&composer, |_, _, cx| cx.notify()),
            cx.subscribe_in(&recipient, window, |this, _, _: &Submitted, window, cx| {
                this.pick_first(window, cx)
            }),
            cx.subscribe(&composer, |this, _, _: &Submitted, cx| this.send(cx)),
            cx.on_app_quit(|this, cx| {
                this.keep_draft(cx);
                async {}
            }),
        ];
        // Denial stops lifting the window for the keyboard and reports it
        // instead; elsewhere the window stays as it is.
        if window.handle_keyboard_inset() {
            subscriptions.push(cx.observe_keyboard_inset(window, |this, window, cx| {
                this.keyboard_moved(window.keyboard_inset(), window, cx)
            }));
        }
        let thread_list = ListState::new(0, ListAlignment::Bottom, dp(600.));
        let view = cx.weak_entity();
        thread_list.set_scroll_handler(move |event, _, cx| {
            if event.visible_range.start < OLDER_MARGIN {
                let _ = view.update(cx, |this, cx| this.load_older(cx));
            }
        });
        let drafts = Drafts::load();
        let (conversations, screen) = match peer {
            Some(peer) => {
                let draft = drafts.get(&peer).unwrap_or_default().to_owned();
                composer.update(cx, |composer, cx| composer.set_text(draft, cx));
                (vec![Conversation::new(peer.clone())], Screen::Thread(peer))
            }
            None => (Vec::new(), Screen::List),
        };
        let samples = Flag::load("samples");
        let conversations = with_samples(conversations, samples.on());
        let font = Self::font(&look, cx);
        let folder = Contacts::folder();
        let people = People::new(folder.as_deref().map(Contacts::load).unwrap_or_default());
        let arrival = Arrival {
            clock: look.tween(0),
            launch: true,
            back: false,
            leaving: None,
        };
        let keyboard = Keyboard {
            from: 0.0,
            to: 0.0,
            motion: look.tween(0),
        };
        Self {
            look,
            font,
            focus_handle: cx.focus_handle(),
            conversations,
            link: Link::Connecting,
            modem: String::new(),
            sim_slot: 0,
            delivery_reports: false,
            blocked: Vec::new(),
            blocked_messages: Vec::new(),
            people,
            search,
            query: String::new(),
            hits: Default::default(),
            recipient,
            composer,
            drafts,
            pins: Peers::load("pins"),
            marked: Peers::load("unread"),
            samples,
            searching: false,
            top_away: ScrollAway::default(),
            overlay: None,
            notice: None,
            screen,
            arrival,
            keyboard,
            composer_height: Rc::new(Cell::new(dp_lp(COMPOSER_HEIGHT))),
            rows: ListState::new(0, ListAlignment::Top, dp(600.)),
            archive: ListState::new(0, ListAlignment::Top, dp(600.)),
            found: ListState::new(0, ListAlignment::Bottom, dp(600.)),
            candidates: ListState::new(0, ListAlignment::Bottom, dp(600.)),
            thread_list,
            shown: Vec::new(),
            archived_shown: Vec::new(),
            found_items: Vec::new(),
            candidate_items: Vec::new(),
            _follow: cx.spawn(async move |this, cx| Self::follow(this, cx).await),
            _contacts: match folder {
                Some(folder) => {
                    cx.spawn(async move |this, cx| Self::follow_contacts(this, folder, cx).await)
                }
                None => Task::ready(()),
            },
            _settings: cx.spawn(async move |this, cx| Self::follow_settings(this, cx).await),
            _subscriptions: subscriptions,
        }
    }

    /// The user's font, with colour emoji first among its fallbacks.
    fn font(look: &Look, cx: &App) -> Font {
        Font {
            fallbacks: Some(FontFallbacks::from_fonts(vec![EMOJI_FONT.into()])),
            ..gpui::font(look.font_family(&cx.text_system().all_font_names()))
        }
    }

    /// Follows dmail for as long as the window lives, across its restarts.
    async fn follow(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        if let Err(error) = Self::follow_dmail(&this, cx).await {
            eprintln!("messages: dmail: {error}");
        }
        let _ = this.update(cx, |view, cx| {
            view.link = Link::Unavailable(NOT_RUNNING);
            cx.notify();
        });
    }

    /// Reaches dmail afresh, after it could not be reached.
    fn reconnect(&mut self, cx: &mut Context<Self>) {
        self.link = Link::Connecting;
        self._follow = cx.spawn(async move |this, cx| Self::follow(this, cx).await);
        cx.notify();
    }

    /// Rereads the contacts whenever their folder changes.
    async fn follow_contacts(this: WeakEntity<Self>, folder: PathBuf, cx: &mut AsyncApp) {
        let mut watch = match Watch::new(&folder) {
            Ok(watch) => watch,
            Err(error) => return eprintln!("messages: following {}: {error}", folder.display()),
        };
        loop {
            if let Err(error) = watch.changed().await {
                return eprintln!("messages: following {}: {error}", folder.display());
            }
            let read = folder.clone();
            let contacts = cx
                .background_spawn(async move { Contacts::load(&read) })
                .await;
            let taken = this.update(cx, |view, cx| {
                view.people = People::new(contacts);
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    /// Takes Denial's look afresh whenever its settings change.
    async fn follow_settings(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let mut watch = match SettingsWatch::new() {
            Ok(watch) => watch,
            Err(error) => return eprintln!("messages: following Denial's settings: {error}"),
        };
        loop {
            if let Err(error) = watch.changed().await {
                return eprintln!("messages: following Denial's settings: {error}");
            }
            let look = cx.background_spawn(async { Look::load() }).await;
            let taken = this.update(cx, |view, cx| view.take_look(look, cx));
            if taken.is_err() {
                return;
            }
        }
    }

    fn take_look(&mut self, look: Look, cx: &mut Context<Self>) {
        self.font = Self::font(&look, cx);
        for input in [&self.search, &self.recipient, &self.composer] {
            input.update(cx, |input, cx| input.restyle(&look, cx));
        }
        self.look = look;
        cx.notify();
    }

    async fn follow_dmail(this: &WeakEntity<Self>, cx: &mut AsyncApp) -> gpui::Result<()> {
        let bus = zbus::Connection::system().await?;
        let dmail = Messages1Proxy::new(&bus).await?;
        let modem = Modem1Proxy::new(&bus).await?;
        let events = Self::events(&dmail, &modem).await?;
        let mut events = pin!(events);
        Self::reload(this, &dmail, &modem, cx).await?;
        while let Some(event) = events.next().await {
            match event {
                Event::Owner(true) => Self::reload(this, &dmail, &modem, cx).await?,
                event => this.update(cx, |view, cx| view.take(event, cx))?,
            }
        }
        Ok(())
    }

    /// Everything dmail tells, as one stream.
    async fn events(
        dmail: &Messages1Proxy<'static>,
        modem: &Modem1Proxy<'static>,
    ) -> zbus::Result<impl Stream<Item = Event>> {
        let owners = dmail
            .inner()
            .receive_owner_changed()
            .await?
            .map(|owner| Event::Owner(owner.is_some()));
        let states = modem
            .receive_state_changed()
            .await
            .then(|change| async move { Event::Modem(change.get().await.unwrap_or_default()) });
        let slots = modem
            .receive_sim_slot_changed()
            .await
            .then(|change| async move { Event::Slot(change.get().await.unwrap_or_default()) });
        let reports = dmail
            .receive_delivery_reports_changed()
            .await
            .then(|change| async move { Event::Reports(change.get().await.unwrap_or_default()) });
        let received = dmail
            .receive_message_received()
            .await?
            .filter_map(|signal| Some(Event::Received(signal.args().ok()?.peer().clone())));
        let changed = dmail.receive_message_changed().await?.filter_map(|signal| {
            let args = signal.args().ok()?;
            Some(Event::Changed(*args.id(), args.state().clone()))
        });
        let read = dmail
            .receive_conversation_read()
            .await?
            .filter_map(|signal| Some(Event::Read(signal.args().ok()?.peer().clone())));
        let deleted = dmail
            .receive_conversation_deleted()
            .await?
            .filter_map(|signal| Some(Event::Deleted(signal.args().ok()?.peer().clone())));
        let archived = dmail
            .receive_conversation_archived()
            .await?
            .filter_map(|signal| {
                let args = signal.args().ok()?;
                Some(Event::Archived(args.peer().clone(), *args.archived()))
            });
        let blocked = dmail
            .receive_blocked_changed()
            .await?
            .map(|_| Event::BlockedChanged);
        Ok(owners
            .or(states)
            .or(slots)
            .or(reports)
            .or(received)
            .or(changed)
            .or(read)
            .or(deleted)
            .or(archived)
            .or(blocked))
    }

    fn take(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            // follow_dmail reloads everything instead.
            Event::Owner(true) => {}
            Event::Owner(false) => self.link = Link::Unavailable(NOT_RUNNING),
            Event::Modem(state) => self.modem = state,
            Event::Slot(slot) => self.sim_slot = slot,
            Event::Reports(enabled) => self.delivery_reports = enabled,
            Event::Received(peer) => self.received(&peer, cx),
            Event::Changed(id, state) => self.changed(id, state, cx),
            Event::Read(peer) => {
                if let Some(index) = self.conversation(&peer) {
                    self.conversations[index].unread = 0;
                }
                self.marked.set(&peer, false);
            }
            Event::Deleted(peer) => {
                if let Some(index) = self.conversation(&peer) {
                    let open = self.open_thread() == Some(index);
                    self.conversations[index].messages.clear();
                    if open {
                        self.thread_list.reset(0);
                    }
                }
                self.refresh_lists(cx);
            }
            Event::Archived(peer, archived) => {
                if let Some(index) = self.conversation(&peer) {
                    self.conversations[index].archived = archived;
                }
                self.refresh_lists(cx);
            }
            Event::BlockedChanged => self.reread_blocked(cx),
        }
        cx.notify();
    }

    /// Reads the conversations, the block list and the modem's state afresh:
    /// at start and whenever dmail comes back.
    async fn reload(
        this: &WeakEntity<Self>,
        dmail: &Messages1Proxy<'static>,
        modem: &Modem1Proxy<'static>,
        cx: &mut AsyncApp,
    ) -> gpui::Result<()> {
        let read = async {
            zbus::Result::Ok((
                conversations(dmail).await?,
                dmail.list_blocked().await?,
                modem.state().await?,
                modem.sim_slot().await?,
                dmail.delivery_reports().await?,
            ))
        };
        let read = read.await;
        this.update(cx, |view, cx| {
            match read {
                Ok((rows, blocked, state, slot, reports)) => {
                    view.link = Link::Ready(dmail.clone());
                    view.modem = state;
                    view.sim_slot = slot;
                    view.delivery_reports = reports;
                    view.blocked = blocked;
                    view.merge(rows);
                    if let Screen::Thread(peer) = view.screen.clone() {
                        view.sync_thread(&peer, None, cx);
                    }
                    if matches!(view.screen, Screen::Settings | Screen::Blocked) {
                        view.read_blocked_messages(cx);
                    }
                }
                Err(error) => {
                    eprintln!("messages: dmail: {error}");
                    view.link = Link::Unavailable(unavailable(&error));
                }
            }
            cx.notify();
        })
    }

    fn conversation(&self, peer: &str) -> Option<usize> {
        self.conversations.iter().position(|c| c.peer == peer)
    }

    /// The conversation the thread screen is for.
    fn open_thread(&self) -> Option<usize> {
        match &self.screen {
            Screen::Thread(peer) => self.conversation(peer),
            _ => None,
        }
    }

    fn message(&self, id: u64) -> Option<&Message> {
        self.conversations
            .iter()
            .flat_map(|c| &c.messages)
            .find(|m| m.id == id)
    }

    /// The text of a message in a thread or in the blocked folder.
    fn message_text(&self, id: u64) -> Option<String> {
        self.message(id).map(|m| m.text.clone()).or_else(|| {
            self.blocked_messages
                .iter()
                .find(|row| row.0 == id)
                .map(|row| row.2.clone())
        })
    }

    fn is_blocked(&self, peer: &str) -> bool {
        self.blocked.iter().any(|number| same_number(number, peer))
    }

    /// Unread in dmail, or marked unread here.
    fn unread(&self, conversation: &Conversation) -> bool {
        conversation.unread > 0 || self.marked.contains(&conversation.peer)
    }

    /// Takes dmail's lists, keeping what is loaded or animating for each peer.
    /// Conversations dmail has no message in stay while open or drafted.
    fn merge(&mut self, rows: Vec<(ConversationRow, bool)>) {
        let mut old = std::mem::take(&mut self.conversations);
        let fresh: Vec<Conversation> = rows
            .into_iter()
            .map(|(row, archived)| {
                let mut fresh = Conversation::from_row(row, archived);
                if let Some(i) = old.iter().position(|c| c.peer == fresh.peer) {
                    let kept = old.swap_remove(i);
                    fresh.messages = kept.messages;
                    fresh.read_fade = kept.read_fade;
                    fresh.complete = kept.complete;
                    fresh.loading = kept.loading;
                }
                // Read here, whatever dmail had when it listed.
                if matches!(&self.screen, Screen::Thread(peer) if *peer == fresh.peer) {
                    fresh.unread = 0;
                }
                fresh
            })
            .collect();
        for conversation in old.iter_mut().filter(|c| !c.sample) {
            conversation.local = true;
            conversation.archived = false;
        }
        old.extend(fresh);
        // dmail lists newest first; the samples and the conversations being
        // written fall into place by time.
        old.sort_by_key(|c| std::cmp::Reverse(c.last_time));
        self.conversations = old;
        self.prune();
    }

    /// Shows the sample conversations or takes them away.
    fn show_samples(&mut self, on: bool, cx: &mut Context<Self>) {
        self.samples.set(on);
        let conversations = std::mem::take(&mut self.conversations);
        self.conversations = with_samples(conversations, on);
        cx.notify();
    }

    /// Drops conversations dmail has no message in, unless open or drafted.
    fn prune(&mut self) {
        let (screen, drafts) = (&self.screen, &self.drafts);
        self.conversations.retain(|c| {
            !c.local
                || c.sample
                || drafts.get(&c.peer).is_some()
                || matches!(screen, Screen::Thread(peer) if *peer == c.peer)
        });
    }

    fn received(&mut self, peer: &str, cx: &mut Context<Self>) {
        if matches!(&self.screen, Screen::Thread(open) if open == peer) {
            self.mark_read(peer, cx);
        }
        self.refresh(peer, cx);
    }

    fn changed(&mut self, id: u64, state: String, cx: &mut Context<Self>) {
        let message = self
            .conversations
            .iter_mut()
            .flat_map(|c| c.messages.iter_mut())
            .find(|m| m.id == id);
        match message {
            Some(message) => message.settle(state),
            // A send whose answer has not come back yet: rereading its thread
            // settles it.
            None => {
                if let Screen::Thread(peer) = self.screen.clone() {
                    self.refresh(&peer, cx);
                }
            }
        }
    }

    fn dmail(&self) -> Option<Messages1Proxy<'static>> {
        match &self.link {
            Link::Ready(dmail) => Some(dmail.clone()),
            _ => None,
        }
    }

    fn is_sample(&self, peer: &str) -> bool {
        self.conversation(peer)
            .is_some_and(|index| self.conversations[index].sample)
    }

    /// Why a message to `peer` would fail now, if it would, with its icon. A
    /// sample's messages go nowhere, so they never fail.
    fn cannot_send_to(&self, peer: &str) -> Option<(Icon, &'static str)> {
        if self.is_sample(peer) {
            None
        } else {
            self.cannot_send()
        }
    }

    /// Adds `text` to a sample conversation as sent, without dmail.
    fn add_sample_message(&mut self, peer: &str, text: String) {
        let Some(index) = self.conversation(peer) else {
            return;
        };
        let open = self.open_thread() == Some(index);
        let mut message = Message::from_row((
            crate::samples::next_id(),
            true,
            text,
            data::now(),
            "sent".into(),
        ));
        message.appear = Some(self.look.tween(motion::NEW_MESSAGE));
        let conversation = &mut self.conversations[index];
        conversation.last_text.clone_from(&message.text);
        conversation.last_time = message.time;
        conversation.last_outgoing = true;
        conversation.messages.push(message);
        if open {
            let at = conversation.messages.len() - 1;
            self.thread_list.splice(at..at, 1);
        }
    }

    /// Why a message would fail now, if it would, with its icon.
    fn cannot_send(&self) -> Option<(Icon, &'static str)> {
        match (&self.link, self.modem.as_str()) {
            (Link::Connecting, _) => Some((Icon::Network, tr("Connecting to the modem service"))),
            (Link::Unavailable(reason), _) => Some((Icon::Alert, reason)),
            (Link::Ready(_), "registered") => None,
            (_, "no-sim") => Some((Icon::Sim, tr("No SIM card"))),
            (_, "locked") => Some((Icon::Lock, tr("The SIM is locked"))),
            (_, "failed") => Some((Icon::Alert, tr("The modem is not working"))),
            _ => Some((Icon::Network, tr("No mobile network"))),
        }
    }

    /// Runs `call` on dmail in the background; a failure is logged as
    /// `doing` and shown as `note`.
    fn call<F>(
        &self,
        doing: &'static str,
        note: &'static str,
        cx: &mut Context<Self>,
        call: impl FnOnce(Messages1Proxy<'static>) -> F + 'static,
    ) where
        F: Future<Output = zbus::Result<()>> + 'static,
    {
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            if let Err(error) = call(dmail).await {
                eprintln!("messages: {doing}: {error}");
                let _ = this.update(cx, |view, cx| view.note(note, cx));
            }
        })
        .detach();
    }

    fn mark_read(&self, peer: &str, cx: &mut Context<Self>) {
        let peer = peer.to_owned();
        self.call(
            "marking read",
            tr("Not marked read"),
            cx,
            async move |dmail| dmail.mark_read(&peer).await,
        );
    }

    /// Reads a thread's newest page from dmail, and older pages until it
    /// holds the message `at`.
    fn load(&self, peer: &str, at: Option<u64>, cx: &mut Context<Self>) {
        let Some(dmail) = self.dmail() else {
            return;
        };
        let peer = peer.to_owned();
        cx.spawn(async move |this, cx| {
            let mut rows: Vec<MessageRow> = Vec::new();
            let read = loop {
                let before = rows.last().map_or(0, |oldest| oldest.3 + 1);
                match dmail.list_messages(&peer, before, PAGE).await {
                    Ok(page) => {
                        let full = page.len() == PAGE as usize;
                        let fresh: Vec<MessageRow> = page
                            .into_iter()
                            .filter(|r| rows.iter().all(|m| m.0 != r.0))
                            .collect();
                        rows.extend(fresh);
                        let found = at.is_none_or(|id| rows.iter().any(|m| m.0 == id));
                        if found || !full {
                            break Ok(full);
                        }
                    }
                    Err(error) => break Err(error),
                }
            };
            let _ = this.update(cx, |view, cx| {
                let full = match read {
                    Ok(full) => full,
                    Err(error) => return eprintln!("messages: reading {peer}: {error}"),
                };
                let Some(index) = view.conversation(&peer) else {
                    return;
                };
                let open = view.open_thread() == Some(index);
                let conversation = &mut view.conversations[index];
                conversation.complete = !full;
                // dmail lists newest first.
                conversation.messages = rows.into_iter().rev().map(Message::from_row).collect();
                if open {
                    view.thread_list.reset(conversation.messages.len());
                    let item_ix =
                        at.and_then(|id| conversation.messages.iter().position(|m| m.id == id));
                    if let Some(item_ix) = item_ix {
                        view.thread_list.scroll_to(ListOffset {
                            item_ix,
                            offset_in_item: px(0.),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the page before the open thread's oldest message.
    fn load_older(&mut self, cx: &mut Context<Self>) {
        let (Some(dmail), Some(index)) = (self.dmail(), self.open_thread()) else {
            return;
        };
        let conversation = &mut self.conversations[index];
        let Some(oldest) = conversation.messages.first() else {
            return;
        };
        if conversation.complete || conversation.loading {
            return;
        }
        // dmail pages by second, older than `before`: asking for the oldest
        // second again, and dropping what is here, misses none sharing it.
        let before = oldest.time + 1;
        let peer = conversation.peer.clone();
        conversation.loading = true;
        cx.spawn(async move |this, cx| {
            let rows = dmail.list_messages(&peer, before, PAGE).await;
            let _ = this.update(cx, |view, cx| view.take_older(&peer, rows, cx));
        })
        .detach();
    }

    fn take_older(
        &mut self,
        peer: &str,
        rows: zbus::Result<Vec<MessageRow>>,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.conversation(peer) else {
            return;
        };
        let open = self.open_thread() == Some(index);
        let conversation = &mut self.conversations[index];
        conversation.loading = false;
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => return eprintln!("messages: reading {peer}: {error}"),
        };
        let full = rows.len() == PAGE as usize;
        let older: Vec<Message> = rows
            .into_iter()
            .rev()
            .filter(|row| conversation.messages.iter().all(|m| m.id != row.0))
            .map(Message::from_row)
            .collect();
        conversation.complete = !full || older.is_empty();
        let count = older.len();
        conversation.messages.splice(0..0, older);
        if open {
            self.thread_list.splice(0..0, count);
        }
        cx.notify();
    }

    /// Rereads the conversation lists: after one was archived, deleted or
    /// unblocked.
    fn refresh_lists(&self, cx: &mut Context<Self>) {
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let rows = conversations(&dmail).await;
            let _ = this.update(cx, |view, cx| match rows {
                Ok(rows) => {
                    view.merge(rows);
                    cx.notify();
                }
                Err(error) => eprintln!("messages: reading conversations: {error}"),
            });
        })
        .detach();
    }

    /// Rereads the conversation lists and the newest messages with `peer`:
    /// after a send, a delete or a received message.
    fn refresh(&self, peer: &str, cx: &mut Context<Self>) {
        let Some(dmail) = self.dmail() else {
            return;
        };
        let peer = peer.to_owned();
        cx.spawn(async move |this, cx| {
            let read = async {
                zbus::Result::Ok((
                    conversations(&dmail).await?,
                    dmail.list_messages(&peer, 0, TAIL).await?,
                ))
            };
            let read = read.await;
            let _ = this.update(cx, |view, cx| match read {
                Ok((rows, newest)) => {
                    view.merge(rows);
                    view.take_newest(&peer, newest);
                    cx.notify();
                }
                Err(error) => eprintln!("messages: reading {peer}: {error}"),
            });
        })
        .detach();
    }

    /// Brings dmail's newest messages with `peer` into its thread: states
    /// settle and new messages animate in. A thread not read yet gets them
    /// when it opens.
    fn take_newest(&mut self, peer: &str, rows: Vec<MessageRow>) {
        let Some(index) = self.conversation(peer) else {
            return;
        };
        let open = self.open_thread() == Some(index);
        let appear = self.look.tween(motion::NEW_MESSAGE);
        let conversation = &mut self.conversations[index];
        if conversation.messages.is_empty() && !open {
            return;
        }
        for row in rows.into_iter().rev() {
            if let Some(message) = conversation.messages.iter_mut().find(|m| m.id == row.0) {
                message.settle(row.4);
                continue;
            }
            let mut message = Message::from_row(row);
            message.appear = Some(appear);
            // The network's time stamps can be older than messages shown.
            let at = conversation
                .messages
                .partition_point(|m| (m.time, m.id) < (message.time, message.id));
            conversation.messages.insert(at, message);
            if open {
                self.thread_list.splice(at..at, 1);
            }
        }
    }

    /// Removes a message dmail deleted, from its thread or the blocked folder.
    fn forget(&mut self, id: u64) {
        self.blocked_messages.retain(|row| row.0 != id);
        let open = self.open_thread();
        for (index, conversation) in self.conversations.iter_mut().enumerate() {
            if let Some(at) = conversation.messages.iter().position(|m| m.id == id) {
                conversation.messages.remove(at);
                if open == Some(index) {
                    self.thread_list.splice(at..at + 1, 0);
                }
                return;
            }
        }
    }

    /// Asks dmail what the search field's text finds; hits for older text
    /// are dropped when they arrive.
    fn find(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().trim().to_owned();
        if query == self.query {
            return;
        }
        self.query.clone_from(&query);
        self.found.reset(0);
        cx.notify();
        if query.is_empty() {
            self.hits = Default::default();
            return;
        }
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let hits = dmail.search(&query, HITS).await;
            let _ = this.update(cx, |view, cx| {
                if view.search.read(cx).text().trim() != query {
                    return;
                }
                match hits {
                    Ok(hits) => view.hits = (query, hits),
                    Err(error) => eprintln!("messages: searching: {error}"),
                }
                view.found.reset(0);
                cx.notify();
            });
        })
        .detach();
    }

    fn reread_blocked(&mut self, cx: &mut Context<Self>) {
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let blocked = dmail.list_blocked().await;
            let _ = this.update(cx, |view, cx| {
                match blocked {
                    Ok(blocked) => view.blocked = blocked,
                    Err(error) => eprintln!("messages: reading the block list: {error}"),
                }
                // Unblocking brings a sender's messages back to its
                // conversation.
                view.refresh_lists(cx);
                if matches!(view.screen, Screen::Settings | Screen::Blocked) {
                    view.read_blocked_messages(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the newest page of the blocked folder.
    fn read_blocked_messages(&self, cx: &mut Context<Self>) {
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let rows = dmail.list_blocked_messages(0, BLOCKED_PAGE).await;
            let _ = this.update(cx, |view, cx| match rows {
                Ok(rows) => {
                    view.blocked_messages = rows;
                    cx.notify();
                }
                Err(error) => eprintln!("messages: reading blocked messages: {error}"),
            });
        })
        .detach();
    }

    /// Keeps the composer's text as the open thread's draft.
    fn keep_draft(&mut self, cx: &App) {
        if let Screen::Thread(peer) = &self.screen {
            self.drafts.set(peer, self.composer.read(cx).text());
        }
    }

    fn open(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        self.open_at(screen, None, false, window, cx);
    }

    /// Shows the conversation with `peer` when asked from outside: by a
    /// notification, or by `messages <peer>` while Messages runs.
    pub fn open_peer(&mut self, peer: String, window: &mut Window, cx: &mut Context<Self>) {
        let peer = self
            .conversations
            .iter()
            .find(|c| same_number(&c.peer, &peer))
            .map_or(peer, |c| c.peer.clone());
        self.open(Screen::Thread(peer), window, cx);
    }

    /// Switches to `screen`: the shown one leaves as the new one arrives, in
    /// reverse when going `back`. A thread opened from a search hit shows the
    /// message `at`.
    fn open_at(
        &mut self,
        screen: Screen,
        at: Option<u64>,
        back: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if screen == self.screen {
            return;
        }
        self.keep_draft(cx);
        self.overlay = None;
        let leaving = std::mem::replace(&mut self.screen, screen.clone());
        self.top_away = ScrollAway::shown(self.scrolled(&screen).unwrap_or(0.0));
        // One thread to another shares the thread's list: it changes at once.
        let threads = matches!((&leaving, &screen), (Screen::Thread(_), Screen::Thread(_)));
        self.arrival = Arrival {
            clock: self.look.tween(0),
            launch: false,
            back,
            leaving: (!threads).then_some(leaving),
        };
        match screen {
            Screen::Thread(peer) => {
                let index = self.conversation(&peer).unwrap_or_else(|| {
                    self.conversations
                        .insert(0, Conversation::new(peer.clone()));
                    0
                });
                self.marked.set(&peer, false);
                let draft = self.drafts.get(&peer).unwrap_or_default().to_owned();
                self.composer
                    .update(cx, |composer, cx| composer.set_text(draft, cx));
                self.thread_list
                    .reset(self.conversations[index].messages.len());
                self.sync_thread(&peer, at, cx);
            }
            Screen::Compose => {
                self.recipient
                    .update(cx, |recipient, cx| recipient.clear(cx));
                window.focus(&self.recipient.focus_handle(cx), cx);
            }
            Screen::Settings | Screen::Blocked => {
                self.read_blocked_messages(cx);
                window.focus(&self.focus_handle, cx);
            }
            // Leaving a thread closes the on-screen keyboard.
            Screen::List | Screen::Archived => window.focus(&self.focus_handle, cx),
        }
        self.prune();
        cx.notify();
    }

    /// Marks the open thread read and reads it from dmail: its newest page,
    /// or back to the message `at`.
    fn sync_thread(&mut self, peer: &str, at: Option<u64>, cx: &mut Context<Self>) {
        let Some(index) = self.conversation(peer) else {
            return;
        };
        // A sample has all its messages and nothing to tell dmail.
        let sample = self.conversations[index].sample;
        if self.dmail().is_none() && !sample {
            return;
        }
        let unread = self.conversations[index].unread;
        if unread > 0 {
            let fade = self.look.tween(motion::READ);
            let conversation = &mut self.conversations[index];
            conversation.unread = 0;
            conversation.read_fade = Some((fade, unread));
            if !sample {
                self.mark_read(peer, cx);
            }
        }
        if !sample {
            self.load(peer, at, cx);
        }
    }

    /// The screen Back goes to.
    fn parent(&self) -> Screen {
        match &self.screen {
            Screen::Thread(_)
                if self
                    .open_thread()
                    .is_some_and(|i| self.conversations[i].archived) =>
            {
                Screen::Archived
            }
            Screen::Blocked => Screen::Settings,
            _ => Screen::List,
        }
    }

    /// Denial's Back gesture, Escape and the top group's back button: undoes
    /// the innermost state. An overlay closes; then search ends and its field
    /// clears; then the screen pops; on the list, the window closes. Drafts
    /// stay.
    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay.is_some() {
            self.dismiss(cx);
        } else if self.screen == Screen::List && self.searching {
            self.searching = false;
            self.search.update(cx, |search, cx| search.clear(cx));
            window.focus(&self.focus_handle, cx);
            cx.notify();
        } else if self.screen == Screen::List {
            window.remove_window();
        } else {
            let parent = self.parent();
            self.open_at(parent, None, true, window, cx);
        }
    }

    /// Who a new conversation can go to, best first: the typed number when it
    /// is new, conversations, then contacts' numbers without one, matching
    /// what is typed by name or number.
    fn find_candidates(&self, cx: &App) -> Vec<Candidate> {
        let typed = self.recipient.read(cx).text().trim();
        let number = dialable(typed);
        let lower = typed.to_lowercase();
        let matches = |name: &str, peer: &str| match &number {
            Some(number) => {
                same_number(peer, number) || peer.contains(number.trim_start_matches('+'))
            }
            None => name.to_lowercase().contains(&lower) || peer.to_lowercase().contains(&lower),
        };
        let mut candidates: Vec<Candidate> = self
            .conversations
            .iter()
            .filter(|c| matches(self.people.name(&c.peer), &c.peer))
            .map(|c| Candidate {
                peer: c.peer.clone(),
                title: self.people.name(&c.peer).to_owned(),
                detail: c.last_text.clone(),
                typed: false,
            })
            .collect();
        for (index, contact) in self.people.contacts().iter().enumerate() {
            for phone in &contact.phones {
                let Some(peer) = dialable(&phone.number) else {
                    continue;
                };
                // A number with a conversation is listed with it.
                let talked = self.conversations.iter().any(|c| {
                    self.people.find(&c.peer) == Some(index) && same_number(&c.peer, &peer)
                });
                if talked || !matches(&contact.name, &peer) {
                    continue;
                }
                candidates.push(Candidate {
                    peer,
                    title: contact.name.clone(),
                    detail: format!("{} · {}", tr(phone.kind.label()), phone.number),
                    typed: false,
                });
            }
        }
        if let Some(number) = number
            && !candidates.iter().any(|c| same_number(&c.peer, &number))
        {
            candidates.insert(
                0,
                Candidate {
                    title: fill("Send to {number}", &[("number", &number)]),
                    peer: number,
                    detail: tr("New conversation").to_string(),
                    typed: true,
                },
            );
        }
        candidates
    }

    fn pick_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(candidate) = self.find_candidates(cx).into_iter().next() {
            self.start(candidate.peer, window, cx);
        }
    }

    /// Opens the thread with `peer` from New conversation, ready to type.
    fn start(&mut self, peer: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open(Screen::Thread(peer), window, cx);
        window.focus(&self.composer.focus_handle(cx), cx);
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Screen::Thread(peer) = self.screen.clone() else {
            return;
        };
        let text = self.composer.read(cx).text().trim().to_owned();
        if text.is_empty() || self.cannot_send_to(&peer).is_some() || !data::replyable(&peer) {
            return;
        }
        if self.is_sample(&peer) {
            self.composer.update(cx, |composer, cx| composer.clear(cx));
            self.drafts.set(&peer, "");
            self.thread_list.scroll_to_end();
            return self.add_sample_message(&peer, text);
        }
        let Some(dmail) = self.dmail() else {
            return;
        };
        self.composer.update(cx, |composer, cx| composer.clear(cx));
        self.drafts.set(&peer, "");
        self.thread_list.scroll_to_end();
        cx.spawn(async move |this, cx| {
            let sent = dmail.send(&peer, &text).await;
            let _ = this.update(cx, |view, cx| match sent {
                // MessageChanged settles its state.
                Ok(_) => view.refresh(&peer, cx),
                Err(error) => view.unsent(&peer, text, error, cx),
            });
        })
        .detach();
    }

    /// dmail refused a message: it goes back into the composer, unless
    /// something new was typed there.
    fn unsent(&mut self, peer: &str, text: String, error: zbus::Error, cx: &mut Context<Self>) {
        eprintln!("messages: sending: {error}");
        let open = matches!(&self.screen, Screen::Thread(open) if open == peer);
        if open && self.composer.read(cx).text().is_empty() {
            self.composer
                .update(cx, |composer, cx| composer.set_text(text, cx));
        }
        self.note(tr("Not sent"), cx);
    }

    /// Sends a failed or undelivered message again, as a new one in place of
    /// it.
    fn retry(&mut self, id: u64, cx: &mut Context<Self>) {
        let Screen::Thread(peer) = self.screen.clone() else {
            return;
        };
        if let Some((_, reason)) = self.cannot_send_to(&peer) {
            return self.note(reason, cx);
        }
        let Some(text) = self.message_text(id) else {
            return;
        };
        if self.is_sample(&peer) {
            self.forget(id);
            return self.add_sample_message(&peer, text);
        }
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let sent = match dmail.send(&peer, &text).await {
                Ok(_) => dmail.delete(id).await,
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |view, cx| {
                match sent {
                    Ok(()) => view.forget(id),
                    Err(error) => {
                        eprintln!("messages: sending again: {error}");
                        view.note(tr("Not sent"), cx);
                    }
                }
                view.refresh(&peer, cx);
            });
        })
        .detach();
    }

    fn delete_message(&mut self, id: u64, cx: &mut Context<Self>) {
        if crate::samples::is_sample_id(id) {
            return self.forget(id);
        }
        let Some(dmail) = self.dmail() else {
            return;
        };
        let peer = self
            .conversations
            .iter()
            .find(|c| c.messages.iter().any(|m| m.id == id))
            .map(|c| c.peer.clone());
        cx.spawn(async move |this, cx| {
            let deleted = dmail.delete(id).await;
            let _ = this.update(cx, |view, cx| {
                match deleted {
                    Ok(()) => view.forget(id),
                    Err(error) => {
                        eprintln!("messages: deleting: {error}");
                        view.note(tr("Not deleted"), cx);
                    }
                }
                if let Some(peer) = peer {
                    view.refresh(&peer, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Deletes the conversation with `peer` in dmail, and its draft and marks
    /// here. A conversation dmail has no message in only loses those; asking
    /// dmail could reach another one with the same number.
    fn delete_conversation(&mut self, peer: String, cx: &mut Context<Self>) {
        self.drafts.set(&peer, "");
        self.pins.set(&peer, false);
        self.marked.set(&peer, false);
        let Some(index) = self.conversation(&peer) else {
            return;
        };
        if self.conversations.remove(index).local {
            return;
        }
        // ConversationDeleted rereads the lists.
        self.call("deleting", tr("Not deleted"), cx, async move |dmail| {
            dmail.delete_conversation(&peer).await
        });
    }

    fn set_archived(&mut self, peer: String, archived: bool, cx: &mut Context<Self>) {
        if let Some(index) = self.conversation(&peer) {
            self.conversations[index].archived = archived;
        }
        self.note(
            if archived {
                tr("Archived")
            } else {
                tr("Back in Messages")
            },
            cx,
        );
        self.call("archiving", tr("Not changed"), cx, async move |dmail| {
            dmail.set_archived(&peer, archived).await
        });
    }

    fn block(&mut self, peer: String, blocked: bool, cx: &mut Context<Self>) {
        self.note(
            if blocked {
                tr("Blocked")
            } else {
                tr("Unblocked")
            },
            cx,
        );
        self.call("blocking", tr("Not changed"), cx, async move |dmail| {
            if blocked {
                dmail.block(&peer).await
            } else {
                dmail.unblock(&peer).await
            }
        });
    }

    /// Unread conversations are marked read; read ones marked unread, here.
    fn toggle_unread(&mut self, peer: String, cx: &mut Context<Self>) {
        let Some(index) = self.conversation(&peer) else {
            return;
        };
        if !self.unread(&self.conversations[index]) {
            return self.marked.set(&peer, true);
        }
        self.marked.set(&peer, false);
        if self.conversations[index].unread > 0 {
            self.conversations[index].unread = 0;
            self.mark_read(&peer, cx);
        }
    }

    fn set_delivery_reports(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.call(
            "setting delivery reports",
            tr("Not changed"),
            cx,
            async move |dmail| dmail.set_delivery_reports(enabled).await,
        );
    }

    fn set_sim_slot(&mut self, slot: u8, cx: &mut Context<Self>) {
        let Some(dmail) = self.dmail() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let set = async {
                let modem = Modem1Proxy::new(dmail.inner().connection()).await?;
                modem.set_sim_slot(slot).await
            };
            if let Err(error) = set.await {
                eprintln!("messages: choosing SIM {slot}: {error}");
                let _ = this.update(cx, |view, cx| view.note(tr("Not changed"), cx));
            }
        })
        .detach();
    }

    /// Lifts `held`, or drops it when it is lifted already.
    fn hold(&mut self, held: Held, cx: &mut Context<Self>) {
        let overlay = Overlay::Held(held);
        self.overlay = match &self.overlay {
            Some((shown, _)) if *shown == overlay => None,
            _ => Some((overlay, self.look.tween(0))),
        };
        cx.notify();
    }

    fn show(&mut self, overlay: Overlay, cx: &mut Context<Self>) {
        self.overlay = Some((overlay, self.look.tween(0)));
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.overlay.take().is_some() {
            cx.notify();
        }
    }

    fn confirmed(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.overlay = None;
        match confirm {
            Confirm::DeleteMessage(id) => self.delete_message(id, cx),
            Confirm::DeleteConversation(peer) => self.delete_conversation(peer, cx),
            Confirm::Block(peer) => self.block(peer, true, cx),
        }
        cx.notify();
    }

    /// A tap on a message: on a link, number or code in it, or anywhere on a
    /// failed one to send it again. Over a long press, it drops the press.
    fn tap_message(&mut self, id: u64, at: Option<usize>, cx: &mut Context<Self>) {
        if self.overlay.is_some() {
            return self.dismiss(cx);
        }
        let Some(message) = self.message(id) else {
            return;
        };
        let kind = at
            .and_then(|at| message.spans.iter().find(|span| span.range.contains(&at)))
            .map(|span| span.kind.clone());
        match kind {
            Some(Kind::Link(url) | Kind::Mail(url)) => cx.open_url(&url),
            Some(Kind::Phone(number)) => self.copy(number, tr("Number copied"), cx),
            Some(Kind::Code(code)) => self.copy(code, tr("Code copied"), cx),
            None if message.failed() => self.retry(id, cx),
            None => {}
        }
    }

    fn copy(&mut self, text: String, note: &'static str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.note(note, cx);
    }

    /// Shows `note` on a chip above the bottom zone for a moment.
    fn note(&mut self, note: impl Into<SharedString>, cx: &mut Context<Self>) {
        let hide = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(motion::NOTICE))
                .await;
            let _ = this.update(cx, |view, cx| {
                view.notice = None;
                cx.notify();
            });
        });
        self.notice = Some((note.into(), self.look.tween(motion::CHIP_FADE), hide));
        cx.notify();
    }

    /// Follows the keyboard to its new height, over the shell's motion there.
    fn keyboard_moved(
        &mut self,
        inset: KeyboardInset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The user put the keyboard away, by dragging it down or with Back:
        // the field it typed into lets go of the focus, so a tap brings it back.
        if inset.height == px(0.) && self.keyboard.to > 0.0 {
            window.focus(&self.focus_handle, cx);
        }
        self.keyboard = Keyboard {
            from: self.keyboard.height(Instant::now()),
            to: f32::from(inset.height),
            motion: self.look.tween(inset.duration.as_millis() as u64),
        };
        cx.notify();
    }

    fn animating(&self, now: Instant) -> bool {
        let tweens = [
            self.overlay
                .as_ref()
                .map(|(_, lift)| lift.segment(0, motion::DIM)),
            self.notice.as_ref().map(|(_, fade, _)| *fade),
            Some(self.keyboard.motion),
        ];
        self.arrival.running(now)
            || tweens.iter().flatten().any(|tween| tween.running(now))
            || self.conversations.iter().any(|conversation| {
                conversation
                    .read_fade
                    .is_some_and(|(fade, _)| fade.running(now))
                    || conversation
                        .messages
                        .iter()
                        .rev()
                        .take(2)
                        .any(|message| message.appear.is_some_and(|appear| appear.running(now)))
            })
    }

    /// How far the rest of the screen is dimmed behind an overlay, 0 to 1.
    fn dim(&self, now: Instant) -> f32 {
        self.overlay
            .as_ref()
            .map_or(0.0, |(_, lift)| lift.segment(0, motion::DIM).progress(now))
    }

    /// Item `index` of a screen's content: where it is while the screen comes
    /// or goes, and dimmed behind an overlay unless it is what the overlay is
    /// about.
    fn item_motion(&self, layer: Layer, index: usize, lifted: bool, now: Instant) -> Motion {
        let motion = match layer {
            Layer::Shown => self.arrival.item(index, now),
            Layer::Leaving => self.arrival.leaving_item(now),
        };
        let dim = if lifted || !layer.live() {
            1.0
        } else {
            denial_look::lerp(1.0, motion::DIM_ALPHA, self.dim(now))
        };
        Motion {
            opacity: motion.opacity * dim,
            ..motion
        }
    }

    /// One screen: its content between the groups, and its bottom zone.
    fn layer(
        &self,
        screen: &Screen,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content = match screen {
            Screen::List if self.searching && !self.search.read(cx).text().trim().is_empty() => {
                self.found_list(layer, cx)
            }
            Screen::List => self.conversation_list(false, layer, now, cx),
            Screen::Archived => self.conversation_list(true, layer, now, cx),
            Screen::Compose => self.candidate_list(layer, cx),
            Screen::Thread(peer) => match self.conversation(peer) {
                Some(index) => self.thread(index, layer, now, cx),
                None => div().into_any_element(),
            },
            Screen::Settings => self.settings(layer, now, cx),
            Screen::Blocked => self.blocked_folder(layer, now, cx),
        };
        let bottom = match layer {
            Layer::Shown => self.arrival.bottom(now),
            Layer::Leaving => {
                let gone = self.arrival.gone(now).unwrap_or(1.0);
                Motion {
                    opacity: 1.0 - gone,
                    offset: motion::BOTTOM_TRAVEL * gone,
                }
            }
        };
        let mut layer_div = div()
            .id(match layer {
                Layer::Shown => "shown",
                Layer::Leaving => "leaving",
            })
            .absolute()
            .inset_0()
            // The content scrolls from the top group's upper edge to the
            // window's lower one, under the groups, the gesture zone and the
            // keyboard. Above the top group the gutter stays on the backing,
            // which Denial paints the status strip with.
            .child(
                div()
                    .absolute()
                    .top(px(INSET))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(content),
            )
            .child(self.bottom_zone(screen, layer, bottom, now, cx));
        if layer.live() {
            layer_div = layer_div.on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)));
        }
        layer_div.into_any_element()
    }

    /// Everything above the gesture zone or the on-screen keyboard at the
    /// bottom: the thread's composer, the chips over it, and a confirmation
    /// in its place. The composer is the one thing that rides on the
    /// keyboard; the thread goes on under it and the keys.
    fn bottom_zone(
        &self,
        screen: &Screen,
        layer: Layer,
        motion: Motion,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let confirm = match &self.overlay {
            Some((Overlay::Confirm(confirm), lift)) if layer.live() => Some((confirm, *lift)),
            _ => None,
        };
        let content = match confirm {
            Some((confirm, lift)) => Some(self.dialog(confirm, lift, now, cx).into_any_element()),
            None => match screen {
                Screen::Thread(peer) => self
                    .conversation(peer)
                    .map(|index| self.composer_zone(index, layer, cx)),
                Screen::List
                | Screen::Compose
                | Screen::Archived
                | Screen::Settings
                | Screen::Blocked => None,
            },
        };
        let notice = self
            .notice
            .as_ref()
            .filter(|_| layer.live())
            .map(|(note, fade, _)| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(gpui::relative(1.))
                    .pb(px(INSET))
                    .flex()
                    .justify_center()
                    .opacity(fade.progress(now))
                    .child(denial_look_gpui::chip(&self.look).child(note.clone()))
            });
        // The composer's height, measured as it is drawn, for the thread's
        // last bubble to clear it.
        let measured = matches!(screen, Screen::Thread(_)) && layer.live() && confirm.is_none();
        let content = content.map(|content| {
            let height = self.composer_height.clone();
            div().relative().child(content).when(measured, |zone| {
                zone.child(
                    canvas(
                        move |bounds, window, _| {
                            let drawn = f32::from(bounds.size.height);
                            if height.replace(drawn) != drawn {
                                window.refresh();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
        });
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(-motion.offset))
            .px(px(INSET))
            .pb(px(bottom_inset(self.keyboard.height(now))))
            .opacity(motion.opacity)
            .children(notice)
            .children(content)
    }

    /// Padding under the last island of a list: clear of the gesture zone,
    /// or of the keyboard while it is up.
    fn list_bottom(&self, now: Instant) -> f32 {
        content_bottom(self.keyboard.height(now), 0.0, 0.0)
    }

    /// Padding under the thread's last bubble: clear of the composer, which
    /// rides on the keyboard.
    fn thread_bottom(&self, now: Instant) -> f32 {
        content_bottom(self.keyboard.height(now), 0.0, self.composer_height.get())
    }

    /// How far `screen`'s conversations are scrolled, lp, on the lists.
    fn scrolled(&self, screen: &Screen) -> Option<f32> {
        let list = match screen {
            Screen::List => &self.rows,
            Screen::Archived => &self.archive,
            _ => return None,
        };
        Some(-f32::from(list.scroll_px_offset_for_scrollbar().y))
    }

    /// Moves the top group with the list's scroll. It stays while searching.
    fn follow_scroll(&mut self) {
        match self.scrolled(&self.screen) {
            Some(offset) if !self.searching => {
                self.top_away.follow(offset, INSET + dp_lp(GROUP_HEIGHT));
            }
            offset => self.top_away = ScrollAway::shown(offset.unwrap_or(0.0)),
        }
    }

    /// The top group: the shown screen's contents over the leaving one's.
    fn top_group(&self, now: Instant, cx: &mut Context<Self>) -> Div {
        let gone = self.arrival.gone(now);
        let leaving = self
            .arrival
            .leaving
            .as_ref()
            .zip(gone)
            .map(|(screen, gone)| {
                let out = self
                    .arrival
                    .clock
                    .segment(0, motion::GROUP_OUT)
                    .progress(now)
                    .max(gone);
                div()
                    .id("leaving-top")
                    .absolute()
                    .inset_0()
                    .opacity(1.0 - out)
                    .child(self.top_contents(screen, Layer::Leaving, cx))
            });
        div()
            .absolute()
            .top(px(INSET - self.top_away.away()))
            .left(px(INSET))
            .right(px(INSET))
            .h(dp(GROUP_HEIGHT))
            .child(
                glass_group(&self.look)
                    .size_full()
                    .relative()
                    .children(leaving)
                    .child(
                        div()
                            .id("shown-top")
                            .absolute()
                            .inset_0()
                            .opacity(self.arrival.group(now))
                            .child(self.top_contents(&self.screen, Layer::Shown, cx)),
                    ),
            )
    }

    /// What the top group holds on `screen`.
    fn top_contents(&self, screen: &Screen, layer: Layer, cx: &mut Context<Self>) -> Div {
        match screen {
            Screen::List => self.list_top(layer, cx),
            Screen::Archived => self.titled_top(tr("Archived"), layer, cx),
            Screen::Compose => self.compose_top(layer, cx),
            Screen::Thread(peer) => match self.conversation(peer) {
                Some(index) => self.thread_top(index, layer, cx),
                None => self.titled_top("", layer, cx),
            },
            Screen::Settings => self.titled_top(tr("Settings"), layer, cx),
            Screen::Blocked => self.titled_top(tr("Blocked messages"), layer, cx),
        }
    }

    /// Takes what each list shows for this frame.
    fn take_items(&mut self, cx: &mut Context<Self>) {
        let drawn = |screen: Screen| {
            self.screen == screen || self.arrival.leaving.as_ref() == Some(&screen)
        };
        let (searching, composing) = (drawn(Screen::List), drawn(Screen::Compose));
        self.shown = self.rows_for(false);
        self.archived_shown = self.rows_for(true);
        self.found_items = if searching {
            self.find_items(cx)
        } else {
            Vec::new()
        };
        self.candidate_items = if composing {
            self.find_candidates(cx)
        } else {
            Vec::new()
        };
        for (state, count) in [
            (&self.rows, self.shown.len()),
            (&self.archive, self.archived_shown.len()),
            (&self.found, self.found_items.len()),
            (&self.candidates, self.candidate_items.len().max(1)),
        ] {
            if state.item_count() != count {
                state.splice(0..state.item_count(), count);
            }
        }
    }
}

/// dmail's conversations, the archived ones marked.
async fn conversations(
    dmail: &Messages1Proxy<'static>,
) -> zbus::Result<Vec<(ConversationRow, bool)>> {
    let inbox = dmail.list_conversations().await?;
    let archived = dmail.list_archived_conversations().await?;
    Ok(inbox
        .into_iter()
        .map(|row| (row, false))
        .chain(archived.into_iter().map(|row| (row, true)))
        .collect())
}

/// What the list says when dmail cannot be reached.
fn unavailable(error: &zbus::Error) -> &'static str {
    match error {
        zbus::Error::MethodError(name, ..) if name.as_str().ends_with(".AccessDenied") => {
            tr("Messages may not use the modem service")
        }
        _ => NOT_RUNNING,
    }
}

impl Render for Messages {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.animating(now) {
            window.request_animation_frame();
        }
        self.take_items(cx);
        self.follow_scroll();
        let leaving = self
            .arrival
            .leaving
            .clone()
            .filter(|_| self.arrival.gone(now).is_some())
            .map(|screen| self.layer(&screen, Layer::Leaving, now, cx));
        let shown = self.layer(&self.screen.clone(), Layer::Shown, now, cx);
        div()
            .size_full()
            .relative()
            .key_context("Messages")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .bg(backing(&self.look))
            .font(self.font.clone())
            .text_color(solid(self.look.palette.foreground))
            .children(leaving)
            .child(shown)
            .child(self.top_group(now, cx))
            .children(self.menu(now, cx))
    }
}

/// `conversations` with the samples added, or with them taken out. A sample
/// takes the place of a conversation being written to its number.
fn with_samples(mut conversations: Vec<Conversation>, on: bool) -> Vec<Conversation> {
    conversations.retain(|c| !c.sample);
    if on {
        let samples = crate::samples::conversations(data::now());
        conversations.retain(|c| !c.local || samples.iter().all(|s| s.peer != c.peer));
        conversations.extend(samples);
        conversations.sort_by_key(|c| std::cmp::Reverse(c.last_time));
    }
    conversations
}
