//! Made-up conversations to try Messages without a SIM or a history, shown
//! when Settings turns them on. They live only in the app: dmail never holds
//! them, and a message written in one is added on screen and sent nowhere.
//! Their numbers are in +44 7700 900000 to 900999, which Ofcom keeps for
//! fiction, so none of them reaches a person.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::data::{Conversation, Message};

/// Sample message ids start here, far above any id dmail gives.
const FIRST_ID: u64 = 1 << 62;
/// The next id for a message written in a sample conversation.
static NEXT_ID: AtomicU64 = AtomicU64::new(FIRST_ID + 1_000_000);

struct Sample {
    peer: &'static str,
    name: &'static str,
    /// Unread messages at the end of `messages`.
    unread: u32,
    /// Text, outgoing, minutes before now, and state.
    messages: &'static [(&'static str, bool, i64, &'static str)],
}

const SAMPLES: &[Sample] = &[
    Sample {
        peer: "+447700900123",
        name: "Giulia Rinaldi",
        unread: 2,
        messages: &[
            ("Are we still on for tonight?", false, 95, "received"),
            ("I can bring dessert", false, 94, "received"),
            ("Yes! 8 at mine", true, 90, "delivered"),
            (
                "Bring the tiramisù, I'll sort the rest",
                true,
                89,
                "delivered",
            ),
            ("Fine. Then I'll do the wine", false, 12, "received"),
            ("Red or white? 🍷", false, 11, "received"),
        ],
    },
    Sample {
        peer: "+447700900456",
        name: "Mamma",
        unread: 1,
        messages: &[
            ("Did you eat?", false, 190, "received"),
            ("Yes, pasta with the sauce you left", true, 182, "sent"),
            ("Good. Call me after work ❤️", false, 41, "received"),
        ],
    },
    Sample {
        peer: "+447700900789",
        name: "Marco Bellini",
        unread: 0,
        messages: &[
            ("The keys are under the mat", false, 1610, "received"),
            ("Thanks, back on Sunday", true, 1602, "delivered"),
            (
                "Take your time. Trains may strike Saturday, times at https://example.org/strike",
                false,
                1590,
                "received",
            ),
        ],
    },
    Sample {
        peer: "+447700900321",
        name: "Sara Conti",
        unread: 0,
        messages: &[
            ("Lunch on Thursday?", false, 4380, "received"),
            ("Sure, the usual place at 1", true, 4371, "sent"),
            ("See you there!", true, 4370, "failed"),
        ],
    },
    Sample {
        peer: "Parcel Point",
        name: "Parcel Point",
        unread: 0,
        messages: &[(
            "Your parcel is ready for pickup at Via Roma 12 until Friday. Pickup code 739204",
            false,
            5800,
            "received",
        )],
    },
];

/// The sample conversations, their times counted back from `now`.
pub fn conversations(now: i64) -> Vec<Conversation> {
    let mut id = FIRST_ID;
    SAMPLES
        .iter()
        .map(|sample| {
            let messages: Vec<Message> = sample
                .messages
                .iter()
                .map(|&(text, outgoing, minutes, state)| {
                    id += 1;
                    let row = (
                        id,
                        outgoing,
                        text.to_owned(),
                        now - minutes * 60,
                        state.to_owned(),
                    );
                    Message::from_row(row)
                })
                .collect();
            let last = messages.last().expect("a sample has messages");
            let row = (
                sample.peer.to_owned(),
                last.text.clone(),
                last.time,
                last.outgoing,
                sample.unread,
            );
            Conversation {
                messages,
                complete: true,
                sample: true,
                ..Conversation::from_row(row, false)
            }
        })
        .collect()
}

/// The made-up name of a sample's peer.
pub fn name(peer: &str) -> Option<&'static str> {
    SAMPLES.iter().find(|s| s.peer == peer).map(|s| s.name)
}

/// An id for a message written in a sample conversation.
pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Whether message `id` belongs to a sample conversation.
pub fn is_sample_id(id: u64) -> bool {
    id >= FIRST_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample can only be answered on a number kept for fiction, and every
    /// message in it has a sample id.
    #[test]
    fn samples_reach_no_one() {
        for conversation in conversations(1_700_000_000) {
            assert!(
                conversation.peer.starts_with("+447700900")
                    || !crate::data::replyable(&conversation.peer),
                "{}",
                conversation.peer
            );
            assert!(conversation.messages.iter().all(|m| is_sample_id(m.id)));
        }
        assert!(is_sample_id(next_id()));
    }
}
