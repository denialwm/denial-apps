//! Sample conversations for the demo. Nothing here talks to a modem.

use denial_look::Tween;

pub struct Message {
    pub text: String,
    pub outgoing: bool,
    pub time: &'static str,
    /// Set for messages added while the app runs, so they animate in.
    pub appear: Option<Tween>,
}

pub struct Conversation {
    pub name: &'static str,
    pub avatar: u32,
    pub unread: bool,
    /// Fades the unread dot after the conversation is opened.
    pub read_fade: Option<Tween>,
    pub messages: Vec<Message>,
    replies: &'static [&'static str],
    next_reply: usize,
}

impl Conversation {
    pub fn initials(&self) -> String {
        self.name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .collect()
    }

    pub fn last(&self) -> &Message {
        self.messages.last().expect("conversation without messages")
    }

    pub fn next_reply(&mut self) -> &'static str {
        let reply = self.replies[self.next_reply % self.replies.len()];
        self.next_reply += 1;
        reply
    }
}

fn thread(lines: &[(bool, &'static str, &'static str)]) -> Vec<Message> {
    lines
        .iter()
        .map(|&(outgoing, time, text)| Message {
            text: text.to_owned(),
            outgoing,
            time,
            appear: None,
        })
        .collect()
}

fn conversation(
    name: &'static str,
    avatar: u32,
    unread: bool,
    lines: &[(bool, &'static str, &'static str)],
    replies: &'static [&'static str],
) -> Conversation {
    Conversation {
        name,
        avatar,
        unread,
        read_fade: None,
        messages: thread(lines),
        replies,
        next_reply: 0,
    }
}

const IN: bool = false;
const OUT: bool = true;

pub fn sample() -> Vec<Conversation> {
    vec![
        conversation(
            "Giulia Romano",
            0x6E5A8A,
            true,
            &[
                (OUT, "18:02", "Are we still on for dinner tomorrow?"),
                (IN, "18:10", "Yes! 20:00 at the usual place"),
                (IN, "18:10", "I booked a table outside"),
                (OUT, "18:12", "Perfect, see you there"),
                (IN, "18:13", "Can you pick up Sara on the way?"),
                (OUT, "18:15", "Sure, what time?"),
                (IN, "18:15", "19:30 at the station"),
                (OUT, "18:16", "Ok"),
                (IN, "19:02", "Also, did you finish the slides for Thursday?"),
                (OUT, "19:20", "Almost. Two more pages"),
                (IN, "19:21", "You said that last week"),
                (OUT, "19:21", "This time it's true"),
                (IN, "19:40", "Ahah sure"),
                (
                    OUT,
                    "20:05",
                    "The tablet finally drives an external monitor",
                ),
                (IN, "20:06", "Over USB-C?"),
                (OUT, "20:06", "Yes, DisplayPort alt mode"),
                (IN, "20:07", "That's huge"),
                (OUT, "20:09", "And the phone runs the same desktop now"),
                (IN, "20:10", "Wait, the Ace 3 too?"),
                (OUT, "20:11", "Touch, keyboard, everything"),
                (IN, "20:12", "You have to show me"),
                (OUT, "21:30", "I'm writing a messages app for it right now"),
                (IN, "21:31", "In Flutter?"),
                (OUT, "21:31", "Rust. It's very light"),
                (
                    IN,
                    "21:47",
                    "Bring the tablet, I want to see Denial running on it",
                ),
            ],
            &["Ahah ok", "It looks so smooth", "Send me a video later?"],
        ),
        conversation(
            "Marco",
            0x4F7A8C,
            true,
            &[
                (IN, "20:15", "Did the new kernel boot?"),
                (
                    OUT,
                    "20:31",
                    "First try. Touch, display and USB-C video out",
                ),
                (IN, "20:33", "No way. Battery?"),
            ],
            &["Nice", "Which release is that?", "I want one too"],
        ),
        conversation(
            "Mamma",
            0x7C6A4E,
            false,
            &[
                (IN, "12:04", "Are you eating properly?"),
                (OUT, "12:20", "Yes mamma"),
                (IN, "12:21", "Come on Sunday, I'm making lasagna"),
                (OUT, "12:22", "I'll be there"),
            ],
            &["Bravo", "Bring your sister", "Call me tonight"],
        ),
        conversation(
            "Lab group",
            0x5B7F62,
            false,
            &[
                (IN, "Yesterday", "Who left the soldering iron on?"),
                (OUT, "Yesterday", "Not me this time"),
                (IN, "Yesterday", "Sure..."),
            ],
            &[
                "We'll check the logs",
                "Fine, fine",
                "Next time it's coffee for everyone",
            ],
        ),
        conversation(
            "Courier",
            0x8A5560,
            false,
            &[(
                IN,
                "Mon",
                "Your parcel will arrive today between 14:00 and 16:00",
            )],
            &["Thanks, we'll keep you posted"],
        ),
        conversation(
            "Sara Bianchi",
            0x4E6585,
            false,
            &[
                (OUT, "Sun", "Thanks for the photos!"),
                (IN, "Sun", "Anytime. The mountain was beautiful"),
            ],
            &["Next trip in November?", "I'll send the rest tonight"],
        ),
        quiet("Luca Ferri", 0x6A7B4F, "Sat", "Football on Wednesday?"),
        quiet(
            "Dentist",
            0x7A5E78,
            "Sat",
            "Reminder: appointment on Tuesday at 9:30",
        ),
        quiet("Elena", 0x8C6B4A, "Fri", "I left the keys under the plant"),
        quiet(
            "Pietro Galli",
            0x4D7470,
            "Fri",
            "Thanks for the help with the router",
        ),
        quiet(
            "Bank",
            0x5E6B8A,
            "Thu",
            "Your card ending 4412 was used online",
        ),
        quiet(
            "Chiara",
            0x8A6474,
            "Thu",
            "Movie night at mine, bring snacks",
        ),
        quiet("Nonna", 0x7D7350, "Wed", "Call me when you can"),
        quiet("Davide Conti", 0x55708A, "Wed", "The PR is merged"),
        quiet("Gym", 0x6B5A4E, "Tue", "Your membership renews next week"),
        quiet("Francesca", 0x5F7D5B, "Mon", "Happy birthday!!"),
        quiet("Andrea", 0x7A5560, "12 Oct", "See you at the conference"),
        quiet(
            "Building admin",
            0x5A6470,
            "10 Oct",
            "Water is off on Friday morning",
        ),
    ]
}

/// A short, already read conversation to fill the list.
fn quiet(name: &'static str, avatar: u32, time: &'static str, text: &'static str) -> Conversation {
    conversation(name, avatar, false, &[(IN, time, text)], &["Ok", "Thanks"])
}
