//! Made-up contacts and calls to try Calls with, shown until the menu hides
//! them. They live only in the app: no card is written for them. Mobile
//! numbers are in +44 7700 900000 to 900999 and landlines in +44 20 7946 0000
//! to 0999, which Ofcom keeps for fiction, so none of them reaches a person.
//! The first ones are the people of Messages' sample conversations, on the
//! same numbers.

use denial_contacts::{Contact, Kind, Phone};

use crate::data::{Call, Direction};

/// Sample call ids start here, below the ids of calls made while Calls runs.
pub const FIRST_ID: u64 = 1;
/// Ids of calls made while Calls runs start here.
pub const FIRST_LIVE_ID: u64 = 1_000;

/// Name, UID, and numbers with what they are for.
type Card = (&'static str, &'static str, &'static [(&'static str, Kind)]);

const CONTACTS: &[Card] = &[
    (
        "Giulia Rinaldi",
        "sample-giulia",
        &[("+44 7700 900123", Kind::Mobile)],
    ),
    (
        "Mamma",
        "sample-mamma",
        &[
            ("+44 7700 900456", Kind::Mobile),
            ("+44 20 7946 0456", Kind::Home),
        ],
    ),
    (
        "Marco Bellini",
        "sample-marco",
        &[("+44 7700 900789", Kind::Mobile)],
    ),
    (
        "Sara Conti",
        "sample-sara",
        &[
            ("+44 7700 900321", Kind::Mobile),
            ("+44 20 7946 0321", Kind::Work),
        ],
    ),
    (
        "Alessandro Greco",
        "sample-alessandro",
        &[("+44 7700 900111", Kind::Mobile)],
    ),
    (
        "Anna Moretti",
        "sample-anna",
        &[("+44 7700 900222", Kind::Mobile)],
    ),
    (
        "Bianca Lodi",
        "sample-bianca",
        &[("+44 7700 900333", Kind::Mobile)],
    ),
    (
        "Chiara Fontana",
        "sample-chiara",
        &[
            ("+44 7700 900444", Kind::Mobile),
            ("+44 20 7946 0444", Kind::Work),
        ],
    ),
    (
        "Davide Russo",
        "sample-davide",
        &[("+44 7700 900555", Kind::Mobile)],
    ),
    (
        "Elena Costa",
        "sample-elena",
        &[("+44 7700 900666", Kind::Mobile)],
    ),
    (
        "Francesca Villa",
        "sample-francesca",
        &[("+44 7700 900777", Kind::Mobile)],
    ),
    (
        "Luca Ferri",
        "sample-luca",
        &[("+44 7700 900888", Kind::Mobile)],
    ),
    (
        "Nonna Rosa",
        "sample-nonna",
        &[("+44 20 7946 0101", Kind::Home)],
    ),
    (
        "Paolo Esposito",
        "sample-paolo",
        &[("+44 7700 900999", Kind::Mobile)],
    ),
    (
        "Roberta Galli",
        "sample-roberta",
        &[("+44 7700 900135", Kind::Mobile)],
    ),
    (
        "Studio Dentistico Marini",
        "sample-dentist",
        &[("+44 20 7946 0880", Kind::Work)],
    ),
    (
        "Tommaso Ricci",
        "sample-tommaso",
        &[("+44 7700 900246", Kind::Mobile)],
    ),
    (
        "Valentina Marino",
        "sample-valentina",
        &[("+44 7700 900357", Kind::Mobile)],
    ),
];

/// Favourites when the user has picked none yet: UIDs.
pub const FAVOURITES: &[&str] = &[
    "sample-mamma",
    "sample-giulia",
    "sample-marco",
    "sample-elena",
];

/// Number, what happened, minutes before now, and seconds talked.
const CALLS: &[(&str, Direction, i64, u64)] = &[
    ("+44 7700 900123", Direction::Missed, 25, 0),
    ("+44 7700 900456", Direction::Incoming, 70, 754),
    ("+44 7700 900975", Direction::Missed, 190, 0),
    ("+44 7700 900789", Direction::Outgoing, 1_500, 245),
    ("+44 20 7946 0321", Direction::Outgoing, 1_560, 62),
    ("+44 7700 900555", Direction::Incoming, 2_900, 1_830),
    ("+44 7700 900666", Direction::Declined, 4_400, 0),
    ("+44 7700 900456", Direction::Outgoing, 5_800, 1_320),
    ("+44 20 7946 0880", Direction::Incoming, 7_100, 95),
    ("+44 7700 900888", Direction::Missed, 8_000, 0),
    ("+44 20 7946 0999", Direction::Incoming, 9_000, 48),
    ("+44 7700 900123", Direction::Outgoing, 11_500, 3_725),
    ("+44 20 7946 0101", Direction::Incoming, 14_200, 1_140),
];

/// The sample contacts.
pub fn contacts() -> Vec<Contact> {
    CONTACTS
        .iter()
        .map(|&(name, uid, phones)| Contact {
            uid: Some(uid.into()),
            name: name.into(),
            phones: phones
                .iter()
                .map(|&(number, kind)| Phone {
                    number: number.into(),
                    kind,
                })
                .collect(),
            photo: None,
        })
        .collect()
}

/// The sample calls, newest first, their times counted back from `now`.
pub fn calls(now: i64) -> Vec<Call> {
    CALLS
        .iter()
        .zip(FIRST_ID..)
        .map(|(&(number, direction, minutes, talked), id)| Call {
            id,
            number: number.into(),
            direction,
            time: now - minutes * 60,
            talked,
            sample: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sample number is one Ofcom keeps for fiction.
    #[test]
    fn samples_reach_no_one() {
        let fiction = |number: &str| {
            let digits = crate::data::digits(number);
            digits.starts_with("447700900") || digits.starts_with("442079460")
        };
        for contact in contacts() {
            assert!(contact.phones.iter().all(|phone| fiction(&phone.number)));
        }
        for call in calls(1_700_000_000) {
            assert!(fiction(&call.number), "{}", call.number);
            assert!(call.id < FIRST_LIVE_ID);
        }
    }
}
