# denial-contacts

The phone's contacts, read from a folder of vCard files:
`$XDG_DATA_HOME/contacts`, else `~/.local/share/contacts`. Messages names its
conversations with them; the Contacts app will write them, and vdirsyncer can
sync the folder with a CardDAV server later. Read-only for now.

- `Contacts::load` reads every `.vcf` file in the folder, vCard 3.0 and 4.0:
  FN (else N), each TEL with its type, and PHOTO inline in base64 or from a
  `file:` URI. Several cards per file are fine. Subfolders one level down
  are read too, as vdirsyncer keeps one collection per subfolder; a contact
  in two collections, by UID, is taken once.
- `Contacts::find` gives the contact a number reaches. `same_number` is the
  rule, the one dmail files conversations by: the last 9 digits match, or the
  whole numbers when either has fewer than 7. `dialable` turns a written
  number into the form dmail takes.
- `Watch` follows the folder and its collections with inotify, their creation
  and removal too, and wakes once per burst of changes, so a sync rereads the
  folder once.

No dependency parses vCards: the subset above is about 270 lines in
`src/vcard.rs`. `async-io` drives the inotify descriptor, so `Watch` works
under GPUI's executors and under `futures_lite::future::block_on`.
