//! The contacts: the folder's, and the samples while they are shown, with
//! their photos ready to draw.

use std::path::Path;
use std::sync::Arc;

use denial_contacts::{Contact, Contacts, Kind, same_number};
use gpui::{Image, ImageFormat, ImageSource};

#[derive(Default)]
pub struct People {
    contacts: Contacts,
    /// Each contact's photo, ready to draw, in `contacts.all()` order.
    photos: Vec<Option<ImageSource>>,
}

impl People {
    /// The contacts in `folder`, with the samples added when `samples` is on.
    pub fn read(folder: Option<&Path>, samples: bool) -> Self {
        let mut contacts = folder.map(Contacts::load).unwrap_or_default();
        if samples {
            contacts.extend(crate::samples::contacts());
        }
        Self::new(contacts)
    }

    fn new(contacts: Contacts) -> Self {
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

    pub fn all(&self) -> &[Contact] {
        self.contacts.all()
    }

    /// The contact `number` reaches, as an index into `all()`.
    pub fn find(&self, number: &str) -> Option<usize> {
        self.contacts.find(number)
    }

    /// What a contact is known by from one reading of the folder to the
    /// next: its UID, else its name.
    pub fn key(&self, index: usize) -> &str {
        let contact = &self.all()[index];
        contact.uid.as_deref().unwrap_or(&contact.name)
    }

    /// The contact with `key`.
    pub fn by_key(&self, key: &str) -> Option<usize> {
        (0..self.all().len()).find(|&index| self.key(index) == key)
    }

    /// The name of whom `number` reaches, else the number.
    pub fn name<'a>(&'a self, number: &'a str) -> &'a str {
        match self.find(number) {
            Some(index) => &self.all()[index].name,
            None => number,
        }
    }

    /// What `number` is for, in the card of whom it reaches.
    pub fn kind(&self, number: &str) -> Option<Kind> {
        let contact = &self.all()[self.find(number)?];
        contact
            .phones
            .iter()
            .find(|phone| same_number(&phone.number, number))
            .map(|phone| phone.kind)
    }

    pub fn photo(&self, index: usize) -> Option<ImageSource> {
        self.photos[index].clone()
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
