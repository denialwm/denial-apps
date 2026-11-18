//! The profile used by the desktop shell and both settings apps.

use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use denial_look_gpui::{Icon, island, say};
use gpui::{
    Context, Div, Focusable, Image, ImageFormat, PathPromptOptions, Task, img, prelude::*, px,
};
use serde_json::{Value, json};

use super::{Dialog, Layer, Settings};

const DOCUMENT: &str = "profile.json";

#[derive(Default, PartialEq, Eq)]
struct Details {
    name: String,
    avatar: String,
}

#[derive(Default)]
pub(super) struct Profile {
    details: Details,
    follow: Option<Task<()>>,
    busy: bool,
}

pub(super) enum Edit {
    Name(String),
    Photo(Arc<Image>),
    DefaultPhoto,
}

fn directory() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        })
        .map(|root| root.join("denial/plugins/denial_desktop/profile"))
}

fn valid_name(name: &str) -> bool {
    name.chars().count() <= 200 && !name.chars().any(|ch| ch <= '\u{1f}' || ch == '\u{7f}')
}

fn valid_avatar(name: &str) -> bool {
    name.strip_prefix("avatar-")
        .and_then(|name| name.strip_suffix(".png"))
        .and_then(|name| name.split_once('-'))
        .is_some_and(|(stamp, pid)| {
            [stamp, pid]
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit()))
        })
}

fn read(directory: &Path) -> io::Result<Details> {
    let path = directory.join(DOCUMENT);
    match path.metadata() {
        Ok(info) if info.len() <= 65_536 => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Details::default()),
        Err(error) => return Err(error),
        Ok(_) => return Ok(Details::default()),
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?).unwrap_or_default();
    if value["schema"] != 1 {
        return Ok(Details::default());
    }
    let name = value["displayName"]
        .as_str()
        .filter(|name| valid_name(name))
        .unwrap_or("")
        .trim();
    let avatar = value["avatar"]
        .as_str()
        .filter(|name| valid_avatar(name))
        .unwrap_or("");
    Ok(Details {
        name: name.to_owned(),
        avatar: avatar.to_owned(),
    })
}

fn save(directory: &Path, edit: Edit) -> io::Result<Details> {
    std::fs::create_dir_all(directory)?;
    let mut profile = read(directory)?;
    let id = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros(),
        std::process::id()
    );
    let temp = directory.join(format!("profile-{id}.tmp"));
    let mut new_image = None;
    let written = (|| {
        match edit {
            Edit::Name(name) => profile.name = name.trim().to_owned(),
            Edit::DefaultPhoto => profile.avatar.clear(),
            Edit::Photo(image) => {
                profile.avatar = format!("avatar-{id}.png");
                new_image = Some(directory.join(&profile.avatar));
                std::fs::write(new_image.as_ref().unwrap(), &image.bytes)?;
            }
        }
        std::fs::write(
            &temp,
            serde_json::to_vec(
                &json!({"schema": 1, "displayName": profile.name, "avatar": profile.avatar}),
            )?,
        )?;
        std::fs::rename(&temp, directory.join(DOCUMENT))
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        if let Some(path) = new_image {
            let _ = std::fs::remove_file(path);
        }
    }
    written.map(|_| profile)
}

fn load_photo(path: &Path) -> image::ImageResult<Arc<Image>> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let photo = reader.decode()?.thumbnail(512, 512);
    let mut png = Cursor::new(Vec::new());
    photo.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(Arc::new(Image::from_bytes(
        ImageFormat::Png,
        png.into_inner(),
    )))
}

impl Settings {
    pub(super) fn follow_profile(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("you") {
            self.profile.follow = None;
            return;
        }
        if self.profile.follow.is_some() {
            return;
        }
        let Some(directory) = directory() else {
            return;
        };
        self.profile.follow = Some(cx.spawn(async move |this, cx| {
            loop {
                let path = directory.clone();
                let profile = cx.background_spawn(async move { read(&path) }).await;
                if this
                    .update(cx, |view, cx| {
                        if let Ok(profile) = profile
                            && !view.profile.busy
                            && view.profile.details != profile
                        {
                            view.profile.details = profile;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
                async_io::Timer::after(Duration::from_secs(1)).await;
            }
        }));
    }

    pub(super) fn profile_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let details = &self.profile.details;
        let account = ["USER", "LOGNAME"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .find(|name| !name.trim().is_empty())
            .unwrap_or_default();
        let live = layer.live() && !self.profile.busy && directory().is_some();
        let account = self
            .row(
                "profile-account",
                Some(Icon::Person),
                "Account name",
                None,
                (true, false),
            )
            .child(self.value(account));
        let display_name = self
            .row("profile-name", None, "Display name", None, (false, true))
            .child(self.value(details.name.clone()))
            .child(self.chevron());
        let display_name = self.live(display_name, live, cx, |this, window, cx| {
            let initial = this.profile.details.name.clone();
            this.output_field
                .update(cx, |field, cx| field.set_text(initial, cx));
            this.show(
                Dialog::TextEdit {
                    title: "Display name",
                    detail: "Leave blank to use your account name.",
                    valid: Rc::new(valid_name),
                    save: Rc::new(|this, name, cx| this.save_profile(Edit::Name(name), cx)),
                },
                cx,
            );
            window.focus(&this.output_field.focus_handle(cx), cx);
        });
        let custom = !details.avatar.is_empty();
        let photo = self
            .row(
                "profile-photo",
                Some(Icon::Photos).filter(|_| !custom),
                "Choose profile photo",
                None,
                (true, !custom),
            )
            .when(custom, |row| {
                row.children(
                    directory()
                        .map(|dir| img(dir.join(&details.avatar)).size(px(48.)).rounded_full()),
                )
            })
            .child(self.chevron());
        let photo = self.live(photo, live, cx, |this, _, cx| this.choose_profile_photo(cx));
        let mut photos = island(&self.look).child(photo);
        if custom {
            let reset = self.row(
                "profile-clear",
                None,
                "Restore default photo",
                None,
                (false, true),
            );
            let reset = self.live(reset, live, cx, |this, _, cx| {
                this.save_profile(Edit::DefaultPhoto, cx)
            });
            photos = photos.child(self.separator(0.)).child(reset);
        }
        vec![
            island(&self.look)
                .child(account)
                .child(self.separator(0.))
                .child(display_name),
            photos,
        ]
    }

    fn choose_profile_photo(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(say("Choose profile photo")),
        });
        cx.spawn(async move |this, cx| {
            let path = match prompt.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => return,
                _ => {
                    let _ = this.update(cx, |view, cx| {
                        view.note("Could not open the file picker", cx)
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let photo = cx.background_spawn(async move { load_photo(&path) }).await;
            let _ = this.update(cx, |view, cx| {
                if view.screen.is_page("you") && view.dialog.is_none() {
                    match photo {
                        Ok(image) => view.show(Dialog::ProfilePhoto { image }, cx),
                        Err(_) => view.note("Could not read the profile photo", cx),
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn save_profile(&mut self, edit: Edit, cx: &mut Context<Self>) {
        let Some(directory) = directory().filter(|_| !self.profile.busy) else {
            return;
        };
        self.profile.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_spawn(async move { save(&directory, edit) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.profile.busy = false;
                match saved {
                    Ok(details) => view.profile.details = details,
                    Err(_) => view.note("Could not save your profile", cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
