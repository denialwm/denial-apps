//! Who a peer is: its contact's name and photo, from the contacts folder.

use std::sync::Arc;

use denial_contacts::{Contact, Contacts};
use gpui::{Image, ImageFormat, ImageSource};

#[derive(Default)]
pub struct People {
    contacts: Contacts,
    /// Each contact's photo, ready to draw, in `contacts.all()` order.
    photos: Vec<Option<ImageSource>>,
}

impl People {
    pub fn new(contacts: Contacts) -> Self {
        let photos = contacts
            .all()
            .iter()
            .map(|contact| {
                let bytes = contact.photo.as_ref()?;
                let image = Image::from_bytes(format(bytes)?, bytes.clone());
                Some(ImageSource::Image(Arc::new(image)))
            })
            .collect();
        Self { contacts, photos }
    }

    pub fn contacts(&self) -> &[Contact] {
        self.contacts.all()
    }

    /// The contact `peer` reaches, as an index into `contacts()`; none for
    /// sender names and unknown numbers.
    pub fn find(&self, peer: &str) -> Option<usize> {
        self.contacts.find(peer)
    }

    /// The contact's name, else a sample's made-up one, else the peer itself.
    pub fn name<'a>(&'a self, peer: &'a str) -> &'a str {
        match self.find(peer) {
            Some(index) => &self.contacts.all()[index].name,
            None => crate::samples::name(peer).unwrap_or(peer),
        }
    }

    pub fn photo(&self, peer: &str) -> Option<ImageSource> {
        self.photos[self.contacts.find(peer)?].clone()
    }
}

/// A picture's format from its first bytes; cards seldom say it right.
fn format(bytes: &[u8]) -> Option<ImageFormat> {
    match bytes {
        [0xFF, 0xD8, ..] => Some(ImageFormat::Jpeg),
        [0x89, b'P', b'N', b'G', ..] => Some(ImageFormat::Png),
        [b'G', b'I', b'F', b'8', ..] => Some(ImageFormat::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageFormat::Webp),
        [b'B', b'M', ..] => Some(ImageFormat::Bmp),
        _ => None,
    }
}
