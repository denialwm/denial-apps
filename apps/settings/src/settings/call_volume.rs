//! Playback volume of the modem's voice path. The DSP exposes discrete
//! levels through callaudio, including while the phone is between calls.

use gpui::Context;
use zbus::fdo::DBusProxy;
use zbus::proxy::CacheProperties;
use zbus::{Connection, proxy};

use super::Settings;

#[proxy(
    interface = "org.callaudio.CallAudio1",
    default_service = "org.callaudio",
    default_path = "/org/callaudio"
)]
trait CallAudio {
    #[zbus(property)]
    fn volume(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn volume_max(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn set_volume(&self, value: u32) -> zbus::Result<()>;
}

struct Volume {
    proxy: CallAudioProxy<'static>,
    step: u32,
    max: u32,
}

impl Volume {
    fn step_at(&self, level: f32) -> u32 {
        (f64::from(level) * f64::from(self.max)).round() as u32
    }

    async fn read(bus: &Connection) -> Option<Self> {
        // Unsupported cards never own the name. Looking up the running
        // owner avoids activating a service that would wait for hardware.
        let owner = DBusProxy::new(bus)
            .await
            .ok()?
            .get_name_owner("org.callaudio".try_into().ok()?)
            .await
            .ok()?;
        let proxy = CallAudioProxy::builder(bus)
            .destination(owner)
            .ok()?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .ok()?;
        let max = proxy.volume_max().await.ok()?;
        let step = proxy.volume().await.ok()?;
        (max > 0 && step <= max).then_some(Self { proxy, step, max })
    }
}

#[derive(Default)]
pub(super) struct State {
    bus: Option<Connection>,
    volume: Option<Volume>,
    reading: bool,
    sending: bool,
    revision: u64,
}

impl State {
    pub(super) fn new(bus: Option<Connection>) -> Self {
        Self {
            bus,
            ..Self::default()
        }
    }

    pub(super) fn level(&self) -> Option<f32> {
        let volume = self.volume.as_ref()?;
        Some(volume.step as f32 / volume.max as f32)
    }
}

impl Settings {
    pub(super) fn read_call_volume(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.call_volume;
        let Some(bus) = state.bus.clone() else { return };
        if state.reading || state.sending {
            return;
        }
        state.reading = true;
        let revision = state.revision;
        cx.spawn(async move |this, cx| {
            let volume = Volume::read(&bus).await;
            let _ = this.update(cx, |view, cx| {
                let state = &mut view.call_volume;
                state.reading = false;
                if state.revision == revision && view.sliding != Some("sound.call".into()) {
                    state.volume = volume;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn set_call_volume(&mut self, level: f32, cx: &mut Context<Self>) {
        let state = &mut self.call_volume;
        let Some(volume) = &mut state.volume else {
            return;
        };
        let mut step = volume.step_at(level);
        if step == volume.step {
            return;
        }
        volume.step = step;
        state.revision += 1;
        cx.notify();
        if state.sending {
            return;
        }
        state.sending = true;
        let proxy = volume.proxy.clone();
        cx.spawn(async move |this, cx| {
            loop {
                let result = proxy.set_volume(step).await;
                let next = this.update(cx, |view, cx| {
                    let state = &mut view.call_volume;
                    let wanted = state.volume.as_ref().map(|v| v.step);
                    if wanted != Some(step) {
                        return wanted;
                    }
                    state.sending = false;
                    if let Err(error) = result {
                        log::warn!("setting in-call volume: {error}");
                        state.volume = None;
                        view.note("Could not change in-call volume", cx);
                        view.read_call_volume(cx);
                    }
                    None
                });
                match next {
                    Ok(Some(wanted)) => step = wanted,
                    _ => return,
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mixer(u32);

    #[zbus::interface(name = "org.callaudio.CallAudio1")]
    impl Mixer {
        #[zbus(property)]
        fn volume(&self) -> u32 {
            self.0
        }
        #[zbus(property)]
        fn volume_max(&self) -> u32 {
            9
        }
        #[zbus(property)]
        fn set_volume(&mut self, step: u32) {
            assert!(step <= 9);
            self.0 = step;
        }
    }

    #[test]
    #[ignore = "requires SETTINGS_TEST_BUS_ADDRESS pointing to a private session bus"]
    fn reads_and_sets_the_modem_volume_between_calls() {
        futures_lite::future::block_on(async {
            let address = std::env::var("SETTINGS_TEST_BUS_ADDRESS").unwrap();
            let client = zbus::connection::Builder::address(address.as_str())
                .unwrap()
                .build()
                .await
                .unwrap();
            assert!(Volume::read(&client).await.is_none());

            let service = zbus::connection::Builder::address(address.as_str())
                .unwrap()
                .serve_at("/org/callaudio", Mixer(2))
                .unwrap()
                .name("org.callaudio")
                .unwrap()
                .build()
                .await
                .unwrap();
            let volume = Volume::read(&client).await.unwrap();
            assert_eq!(volume.step, 2);
            for (level, expected) in [(0.0, 0), (0.5, 5), (1.0, 9)] {
                volume
                    .proxy
                    .set_volume(volume.step_at(level))
                    .await
                    .unwrap();
                assert_eq!(Volume::read(&client).await.unwrap().step, expected);
            }
            service.release_name("org.callaudio").await.unwrap();
            assert!(Volume::read(&client).await.is_none());
        });
    }
}
