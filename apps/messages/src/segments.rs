//! How many SMS a text takes, split the way dmail splits it (Phonewave's
//! services/modem/dmail/src/sms/pdu.rs): GSM 7-bit when every character is in
//! the default alphabet or its extension, else UCS-2. A part never ends inside
//! an extension character's escape pair or a surrogate pair.

/// The GSM 7-bit default alphabet (3GPP TS 23.038), without its escape.
const BASIC: &str = "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?\
                     ¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà";
/// Characters of the extension table: two septets each.
const EXTENSION: &str = "\u{c}^{}\\[~]|€";

/// Septets or UTF-16 units: a lone message, and each part of a long one.
const GSM_SINGLE: usize = 160;
const GSM_PART: usize = 153;
const UCS2_SINGLE: usize = 70;
const UCS2_PART: usize = 67;

pub struct Count {
    pub parts: usize,
    /// Characters of the narrowest kind still free in the last part.
    pub left: usize,
}

pub fn count(text: &str) -> Count {
    let gsm: Option<Vec<usize>> = text
        .chars()
        .map(|c| {
            if BASIC.contains(c) {
                Some(1)
            } else {
                EXTENSION.contains(c).then_some(2)
            }
        })
        .collect();
    let (widths, single, part) = match gsm {
        Some(widths) => (widths, GSM_SINGLE, GSM_PART),
        None => (
            text.chars().map(char::len_utf16).collect(),
            UCS2_SINGLE,
            UCS2_PART,
        ),
    };
    let total: usize = widths.iter().sum();
    if total <= single {
        return Count {
            parts: 1,
            left: single - total,
        };
    }
    let (mut parts, mut used) = (1, 0);
    for width in widths {
        if used + width > part {
            parts += 1;
            used = 0;
        }
        used += width;
    }
    Count {
        parts,
        left: part - used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts_left(text: &str) -> (usize, usize) {
        let count = count(text);
        (count.parts, count.left)
    }

    #[test]
    fn gsm_splits_at_160_then_153() {
        assert_eq!(parts_left("Ciao!"), (1, 155));
        assert_eq!(parts_left(&"a".repeat(160)), (1, 0));
        assert_eq!(parts_left(&"a".repeat(161)), (2, 145));
        assert_eq!(parts_left(&"a".repeat(306)), (2, 0));
        assert_eq!(parts_left(&"a".repeat(307)), (3, 152));
    }

    #[test]
    fn extension_characters_take_two_and_stay_whole() {
        assert_eq!(parts_left("€"), (1, 158));
        // 152 septets, then a euro that would straddle the boundary.
        let text = format!("{}€", "a".repeat(152)) + &"a".repeat(10);
        assert_eq!(parts_left(&text), (2, 153 - 12));
    }

    #[test]
    fn anything_else_is_ucs2() {
        assert_eq!(parts_left("città è già così"), (1, 160 - 16));
        assert_eq!(parts_left("ciao 😀"), (1, 70 - 7));
        assert_eq!(parts_left(&"ж".repeat(70)), (1, 0));
        assert_eq!(parts_left(&"ж".repeat(71)), (2, 63));
        // A surrogate pair never splits: 66 units, then an emoji.
        assert_eq!(parts_left(&format!("{}😀", "ж".repeat(66 + 67))), (3, 65));
    }
}
