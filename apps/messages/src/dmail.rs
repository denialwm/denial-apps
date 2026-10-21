//! dmail's `org.dmail.Messages1`, the phone's SMS store and sending, and the
//! modem state from `org.dmail.Modem1`. See Phonewave's
//! services/modem/dmail/DESIGN.md.

use zbus::proxy;

/// Peer; the last message's text, time and whether it was outgoing; the unread
/// count.
pub type ConversationRow = (String, String, i64, bool, u32);
/// Id, outgoing, text, time and state: received; or sending, sent, failed,
/// delivered, undelivered or expired.
pub type MessageRow = (u64, bool, String, i64, String);
/// A search hit: the peer, then the message as in `MessageRow`.
pub type FoundRow = (String, u64, bool, String, i64, String);
/// A message from a blocked sender: id, sender, text, time.
pub type BlockedRow = (u64, String, String, i64);

#[proxy(
    interface = "org.dmail.Messages1",
    default_service = "org.dmail",
    default_path = "/org/dmail"
)]
pub trait Messages1 {
    /// Whether sent messages ask the network for delivery reports.
    #[zbus(property)]
    fn delivery_reports(&self) -> zbus::Result<bool>;

    /// Saved; applies to messages sent from then on.
    fn set_delivery_reports(&self, enabled: bool) -> zbus::Result<()>;

    /// Newest first, archived ones left out.
    fn list_conversations(&self) -> zbus::Result<Vec<ConversationRow>>;

    /// The archived conversations, newest first.
    fn list_archived_conversations(&self) -> zbus::Result<Vec<ConversationRow>>;

    /// Up to `limit` messages older than `before` (0 for the newest), newest
    /// first.
    fn list_messages(&self, peer: &str, before: i64, limit: u32) -> zbus::Result<Vec<MessageRow>>;

    /// Stores the message as sending and returns its id; MessageChanged
    /// follows.
    fn send(&self, peer: &str, text: &str) -> zbus::Result<u64>;

    /// ConversationRead follows.
    fn mark_read(&self, peer: &str) -> zbus::Result<()>;

    /// A message in a conversation or in the blocked folder.
    fn delete(&self, id: u64) -> zbus::Result<()>;

    /// ConversationDeleted follows.
    fn delete_conversation(&self, peer: &str) -> zbus::Result<()>;

    /// ConversationArchived follows. A new message brings it back.
    fn set_archived(&self, peer: &str, archived: bool) -> zbus::Result<()>;

    /// Messages with a word starting with each word of `text`, newest first.
    fn search(&self, text: &str, limit: u32) -> zbus::Result<Vec<FoundRow>>;

    /// BlockedChanged follows.
    fn block(&self, number: &str) -> zbus::Result<()>;

    /// The sender's messages go back to its conversation, read.
    /// BlockedChanged follows.
    fn unblock(&self, number: &str) -> zbus::Result<()>;

    fn list_blocked(&self) -> zbus::Result<Vec<String>>;

    /// The blocked folder, newest first, older than `before` (0 for the
    /// newest).
    fn list_blocked_messages(&self, before: i64, limit: u32) -> zbus::Result<Vec<BlockedRow>>;

    #[zbus(signal)]
    fn message_received(&self, id: u64, peer: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn message_changed(&self, id: u64, state: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn conversation_read(&self, peer: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn conversation_deleted(&self, peer: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn conversation_archived(&self, peer: String, archived: bool) -> zbus::Result<()>;

    #[zbus(signal)]
    fn blocked_changed(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.dmail.Modem1",
    default_service = "org.dmail",
    default_path = "/org/dmail"
)]
pub trait Modem1 {
    /// off, starting, no-sim, locked, searching, registered or failed. Only
    /// `registered` sends.
    #[zbus(property)]
    fn state(&self) -> zbus::Result<String>;

    /// The SIM for SMS and data, 1 or 2.
    #[zbus(property)]
    fn sim_slot(&self) -> zbus::Result<u8>;

    /// Saved and applied at once.
    fn set_sim_slot(&self, slot: u8) -> zbus::Result<()>;
}
