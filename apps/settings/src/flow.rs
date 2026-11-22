//! Where the power goes right now: what the charger delivers, what the
//! battery takes or gives, and from the two what the device uses. Read from
//! the kernel's power supplies (`/sys/class/power_supply`), which measure
//! both ends; UPower reports the battery's rate alone and only when it
//! polls. A reading jumps with every burst of work, so the page shows the
//! mean of the last few.

use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// How often the supplies are read while the page shows.
pub const EVERY: Duration = Duration::from_millis(500);
/// The readings one shown figure is the mean of: two seconds' worth.
const WINDOW: usize = 4;

const SUPPLIES: &str = "/sys/class/power_supply";
/// Supplies of the USB Type-C drivers (UCSI, TCPM): their voltage and
/// current are the contract with the charger, not what flows.
const CONTRACTS: [&str; 2] = ["ucsi-source-psy-", "tcpm-source-psy-"];

/// One reading, in watts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// What the charger delivers; None with no charger connected.
    pub charger: Option<f64>,
    /// Into the battery; negative, out of it.
    pub battery: f64,
}

impl Sample {
    /// What the device uses: all the charger delivers that does not reach
    /// the battery (the charging circuit's loss with it), or all the battery
    /// gives.
    pub fn used(&self) -> f64 {
        (self.charger.unwrap_or(0.0) - self.battery).max(0.0)
    }
}

/// The last readings.
#[derive(Default)]
pub struct Flow {
    samples: VecDeque<Sample>,
}

impl Flow {
    /// Takes a reading in; None, when the supplies could not be read,
    /// forgets the earlier ones. So does a charger coming or going: figures
    /// from before say nothing about now.
    pub fn push(&mut self, sample: Option<Sample>) {
        let Some(sample) = sample else {
            self.samples.clear();
            return;
        };
        if self
            .samples
            .back()
            .is_some_and(|last| last.charger.is_some() != sample.charger.is_some())
        {
            self.samples.clear();
        }
        if self.samples.len() == WINDOW {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// The mean of the readings held, if there are any.
    pub fn shown(&self) -> Option<Sample> {
        let count = self.samples.len() as f64;
        let last = self.samples.back()?;
        let mean = |of: fn(&Sample) -> f64| self.samples.iter().map(of).sum::<f64>() / count;
        Some(Sample {
            charger: last
                .charger
                .map(|_| mean(|sample| sample.charger.unwrap_or(0.0))),
            battery: mean(|sample| sample.battery),
        })
    }
}

/// Reads the supplies now; None without a battery that says what flows.
pub fn read() -> Option<Sample> {
    read_at(Path::new(SUPPLIES))
}

fn read_at(supplies: &Path) -> Option<Sample> {
    let (mut battery, mut charger) = (None::<f64>, None::<f64>);
    for entry in fs::read_dir(supplies).ok()?.flatten() {
        let supply = entry.path();
        let text = |name: &str| {
            fs::read_to_string(supply.join(name))
                .ok()
                .map(|text| text.trim().to_owned())
        };
        let number = |name: &str| text(name)?.parse::<f64>().ok();
        // Micro-volts times micro-amperes, or micro-watts.
        let watts = match (number("voltage_now"), number("current_now")) {
            (Some(volts), Some(amperes)) => Some(volts * amperes / 1e12),
            _ => number("power_now").map(|watts| watts / 1e6),
        };
        match text("type").as_deref() {
            Some("Battery") => {
                // A mouse's or a pen's battery is not the device's.
                if text("scope").as_deref() == Some("Device") || number("present") == Some(0.0) {
                    continue;
                }
                let Some(watts) = watts else { continue };
                // Charging counts up here whichever sign the driver gives.
                let watts = match text("status").as_deref() {
                    Some("Discharging") => -watts.abs(),
                    Some("Charging") => watts.abs(),
                    _ => watts,
                };
                *battery.get_or_insert(0.0) += watts;
            }
            Some(_) => {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if CONTRACTS.iter().any(|prefix| name.starts_with(prefix))
                    || number("online").unwrap_or(0.0) <= 0.0
                {
                    continue;
                }
                *charger.get_or_insert(0.0) += watts.unwrap_or(0.0).max(0.0);
            }
            None => {}
        }
    }
    Some(Sample {
        charger,
        battery: battery?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A directory of power supplies as the kernel shows them, removed when
    /// dropped.
    struct Supplies(PathBuf);

    impl Supplies {
        fn new(test: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("settings-flow-{}-{test}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn supply(&self, name: &str, files: &[(&str, &str)]) -> &Self {
            let supply = self.0.join(name);
            fs::create_dir_all(&supply).unwrap();
            for (file, text) in files {
                fs::write(supply.join(file), format!("{text}\n")).unwrap();
            }
            self
        }

        fn read(&self) -> Option<Sample> {
            read_at(&self.0)
        }
    }

    impl Drop for Supplies {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn near(left: f64, right: f64) -> bool {
        (left - right).abs() < 0.001
    }

    /// The OnePlus Pad 3 on a 9 V charger, as its kernel reported it: the
    /// battery manager measures, the Type-C supply names the contract.
    #[test]
    fn charging_splits_the_chargers_power() {
        let supplies = Supplies::new("charging");
        supplies
            .supply(
                "qcom-battmgr-bat",
                &[
                    ("type", "Battery"),
                    ("present", "1"),
                    ("status", "Charging"),
                    ("voltage_now", "4400000"),
                    ("current_now", "2000000"),
                ],
            )
            .supply(
                "qcom-battmgr-usb",
                &[
                    ("type", "USB"),
                    ("online", "1"),
                    ("voltage_now", "8650000"),
                    ("current_now", "1980000"),
                ],
            )
            .supply("qcom-battmgr-wls", &[("type", "Wireless"), ("online", "0")])
            .supply(
                "ucsi-source-psy-pmic_glink.ucsi.01",
                &[
                    ("type", "USB"),
                    ("online", "1"),
                    ("voltage_now", "9000000"),
                    ("current_now", "3000000"),
                ],
            );
        let sample = supplies.read().unwrap();
        assert!(near(sample.charger.unwrap(), 17.127));
        assert!(near(sample.battery, 8.8));
        assert!(near(sample.used(), 8.327));
    }

    #[test]
    fn on_battery_all_it_gives_is_used() {
        let supplies = Supplies::new("draining");
        supplies
            .supply(
                "qcom-battmgr-bat",
                &[
                    ("type", "Battery"),
                    ("present", "1"),
                    ("status", "Discharging"),
                    ("voltage_now", "4301128"),
                    ("current_now", "-1610156"),
                ],
            )
            .supply(
                "qcom-battmgr-usb",
                &[
                    ("type", "USB"),
                    ("online", "0"),
                    ("voltage_now", "71000"),
                    ("current_now", "0"),
                ],
            );
        let sample = supplies.read().unwrap();
        assert_eq!(sample.charger, None);
        assert!(near(sample.battery, -6.925));
        assert!(near(sample.used(), 6.925));
    }

    /// Drivers differ in the sign of a draining battery's current, and some
    /// report the power alone.
    #[test]
    fn status_settles_the_sign() {
        let supplies = Supplies::new("sign");
        supplies.supply(
            "BAT0",
            &[
                ("type", "Battery"),
                ("status", "Discharging"),
                ("power_now", "5500000"),
            ],
        );
        assert!(near(supplies.read().unwrap().battery, -5.5));
    }

    /// The charger carries the device while the battery is held: nothing is
    /// lost by a charger whose current is not reported.
    #[test]
    fn a_charger_without_a_reading_delivers_nothing_known() {
        let supplies = Supplies::new("mains");
        supplies
            .supply(
                "BAT0",
                &[
                    ("type", "Battery"),
                    ("status", "Not charging"),
                    ("voltage_now", "4000000"),
                    ("current_now", "-50000"),
                ],
            )
            .supply("AC", &[("type", "Mains"), ("online", "1")])
            .supply(
                "hid-pen-battery",
                &[
                    ("type", "Battery"),
                    ("scope", "Device"),
                    ("status", "Discharging"),
                    ("voltage_now", "3700000"),
                    ("current_now", "100000"),
                ],
            );
        let sample = supplies.read().unwrap();
        assert_eq!(sample.charger, Some(0.0));
        assert!(near(sample.battery, -0.2));
        assert!(near(sample.used(), 0.2));
    }

    #[test]
    fn no_battery_no_reading() {
        let supplies = Supplies::new("none");
        supplies.supply("AC", &[("type", "Mains"), ("online", "1")]);
        assert_eq!(supplies.read(), None);
        assert_eq!(read_at(&supplies.0.join("missing")), None);
    }

    #[test]
    fn shows_the_mean_of_the_last_readings() {
        let on_battery = |watts: f64| {
            Some(Sample {
                charger: None,
                battery: -watts,
            })
        };
        let mut flow = Flow::default();
        assert_eq!(flow.shown(), None);
        for watts in [9.0, 2.0, 4.0, 6.0, 8.0] {
            flow.push(on_battery(watts));
        }
        // The first has left the window.
        let shown = flow.shown().unwrap();
        assert!(near(shown.battery, -5.0));
        assert!(near(shown.used(), 5.0));
        assert_eq!(shown.charger, None);

        // A charger arrives: what the battery gave before no longer counts.
        flow.push(Some(Sample {
            charger: Some(18.0),
            battery: 10.0,
        }));
        flow.push(Some(Sample {
            charger: Some(16.0),
            battery: 9.0,
        }));
        let shown = flow.shown().unwrap();
        assert!(near(shown.charger.unwrap(), 17.0));
        assert!(near(shown.battery, 9.5));
        assert!(near(shown.used(), 7.5));

        flow.push(None);
        assert_eq!(flow.shown(), None);
    }
}
