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
    // A long thread, to try scrolling: six evenings of planning a trip.
    Sample {
        peer: "+447700900555",
        name: "Davide Russo",
        unread: 0,
        messages: &[
            (
                "Hey, are you free the second weekend of November?",
                false,
                6034,
                "received",
            ),
            ("Should be, why?", true, 6031, "delivered"),
            (
                "Thinking of a few days in the Dolomites before the snow gets serious",
                false,
                6030,
                "received",
            ),
            ("Cortina, or somewhere quieter", false, 6029, "received"),
            ("Oh I'm in", true, 6026, "delivered"),
            (
                "Quieter please, Cortina in November is all closed shutters",
                true,
                6025,
                "delivered",
            ),
            ("Val di Funes then?", false, 6024, "received"),
            (
                "There's a hut that's open until the 20th",
                false,
                6021,
                "received",
            ),
            (
                "The one with the church in every photo?",
                true,
                6020,
                "delivered",
            ),
            ("That one 😄", false, 6019, "received"),
            ("Ok how do we get there", true, 6016, "delivered"),
            ("Train to Bolzano, then the bus", false, 6015, "received"),
            ("Or we drive and split petrol", false, 6014, "received"),
            (
                "Driving means we can stop wherever",
                true,
                6011,
                "delivered",
            ),
            ("And I can bring the big camera", true, 6010, "delivered"),
            ("Fine, driving. Your car or mine?", false, 6009, "received"),
            ("Mine has winter tyres already", true, 6006, "delivered"),
            ("Perfect", false, 6005, "received"),
            (
                "I'll look at places to sleep tonight",
                false,
                6004,
                "received",
            ),
            ("Found three options", false, 4594, "received"),
            ("1. The hut, €65 each with dinner", false, 4591, "received"),
            (
                "2. A guesthouse in St. Magdalena, €80 for the room",
                false,
                4590,
                "received",
            ),
            (
                "3. An apartment in Funes, €110 a night, sleeps four",
                false,
                4589,
                "received",
            ),
            ("Four? Who else is coming", true, 4586, "delivered"),
            ("I asked Elena and Marco", false, 4585, "received"),
            (
                "Elena said maybe, Marco said yes if we leave Friday after 3",
                false,
                4584,
                "received",
            ),
            ("Friday after 3 works", true, 4581, "delivered"),
            ("I'll take the afternoon off", true, 4580, "delivered"),
            (
                "So the apartment if it's four of us, the hut if it's two",
                false,
                4579,
                "received",
            ),
            ("Can we decide by Wednesday?", true, 4576, "delivered"),
            (
                "The apartment holds it until Thursday",
                false,
                4575,
                "received",
            ),
            ("Ok", true, 4574, "delivered"),
            ("What's the plan once we're there", true, 4571, "delivered"),
            (
                "Saturday the Adolf Munkel trail, it's flat-ish",
                false,
                4570,
                "received",
            ),
            (
                "Sunday up to Seceda if the cable car still runs",
                false,
                4569,
                "received",
            ),
            ("It closes on the 2nd I think", true, 4566, "delivered"),
            ("Ugh", false, 4565, "received"),
            (
                "Then Sunday is Rasciesa from Ortisei",
                false,
                4564,
                "received",
            ),
            (
                "That's fine, the view is basically the same",
                true,
                4561,
                "delivered",
            ),
            ("Liar", false, 4560, "received"),
            ("Ok almost the same", true, 4559, "delivered"),
            ("Elena's out, she has a wedding", false, 3154, "received"),
            ("Shame", true, 3151, "delivered"),
            ("So three of us. Apartment still?", false, 3150, "received"),
            (
                "Apartment. Splitting €110 three ways beats the hut",
                true,
                3149,
                "delivered",
            ),
            ("And we can cook", false, 3146, "received"),
            (
                "You can cook, I'll do the washing up",
                true,
                3145,
                "delivered",
            ),
            ("Deal", false, 3144, "received"),
            ("Booked! Friday to Monday", false, 3141, "received"),
            ("Monday too?", true, 3140, "delivered"),
            (
                "We'd leave Monday morning, you said you're off",
                false,
                3139,
                "received",
            ),
            ("I said Friday afternoon", true, 3136, "delivered"),
            ("But ok, I'll move a meeting", true, 3135, "delivered"),
            ("🙏", false, 3134, "received"),
            ("Do they want a deposit?", true, 3131, "delivered"),
            ("Already paid, you owe me €73.30", false, 3130, "received"),
            ("Sending it now", true, 3129, "delivered"),
            ("Done", true, 3126, "delivered"),
            ("Got it, thanks", false, 3125, "received"),
            (
                "Marco asks if he can bring his dog",
                false,
                3124,
                "received",
            ),
            ("Is the apartment ok with dogs?", true, 3121, "delivered"),
            ("Small dogs yes", false, 3120, "received"),
            ("Biscotto is not small", true, 3119, "delivered"),
            ("Biscotto is a horse", false, 3116, "received"),
            ("I'll ask them", true, 3115, "delivered"),
            (
                "They said yes for €15 extra cleaning",
                true,
                1714,
                "delivered",
            ),
            ("Marco will pay that", false, 1711, "received"),
            ("Obviously", true, 1710, "delivered"),
            ("What do we need for the trails?", false, 1709, "received"),
            (
                "Proper boots, it'll be icy in the shade",
                true,
                1706,
                "delivered",
            ),
            ("Microspikes if you have them", true, 1705, "delivered"),
            ("I have the ones from last year", false, 1704, "received"),
            ("Head torch, it's dark by 5", true, 1701, "delivered"),
            ("Right", false, 1700, "received"),
            ("Food for the car?", false, 1699, "received"),
            ("Marco always brings focaccia", true, 1696, "delivered"),
            ("Then we're covered", false, 1695, "received"),
            (
                "Weather says sun on Saturday, clouds Sunday",
                true,
                1694,
                "delivered",
            ),
            (
                "Clouds are fine, as long as no rain",
                false,
                1691,
                "received",
            ),
            ("20% rain Sunday afternoon", true, 1690, "delivered"),
            ("We'll be down by then", false, 1689, "received"),
            ("Leaving at 3 sharp?", true, 1686, "delivered"),
            (
                "3 sharp from mine, Marco comes to me",
                false,
                1685,
                "received",
            ),
            ("Ok. I'll fill up Thursday night", true, 1684, "delivered"),
            (
                "Did you get the chains just in case?",
                false,
                1681,
                "received",
            ),
            ("In the boot since October", true, 1680, "delivered"),
            ("Of course they are", false, 1679, "received"),
            ("Someone has to be the grown-up", true, 1676, "delivered"),
            ("Fair", false, 1675, "received"),
            (
                "Marco says traffic on the A22 is bad on Fridays",
                false,
                274,
                "received",
            ),
            (
                "We'll take the old road after Trento",
                true,
                271,
                "delivered",
            ),
            ("That adds an hour", false, 270, "received"),
            ("But it's pretty", true, 269, "delivered"),
            (
                "It's dark at 5, you said so yourself",
                false,
                266,
                "received",
            ),
            ("Ok the motorway", true, 265, "delivered"),
            ("Check-in is until 9 so no stress", false, 264, "received"),
            ("Do they leave the key in a box?", true, 261, "delivered"),
            ("Code by text the day before", false, 260, "received"),
            ("Perfect", true, 259, "delivered"),
            ("Should we book a dinner Saturday?", true, 256, "delivered"),
            (
                "The Ranuimüller place, I'll call them",
                false,
                255,
                "received",
            ),
            (
                "Booked for 8, table for three plus Biscotto",
                false,
                254,
                "received",
            ),
            ("😂", true, 251, "delivered"),
            ("Can't wait honestly", true, 250, "delivered"),
            ("Me neither", false, 249, "received"),
            ("First holiday since August", false, 246, "received"),
            (
                "Send me the address so I can put it in the maps",
                true,
                245,
                "delivered",
            ),
            ("Via San Pietro 14, Funes", false, 244, "received"),
            ("Saved", true, 241, "delivered"),
            ("See you Friday!", false, 240, "received"),
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
