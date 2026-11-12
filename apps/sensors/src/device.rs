//! What kind of device this is, from systemd-hostnamed's `Chassis`: a tablet
//! says "tablet", a phone "handset". Devicetree devices set it with
//! `chassis-type`.

use zbus::Connection;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;

const NAME: &str = "org.freedesktop.hostname1";
const PATH: &str = "/org/freedesktop/hostname1";

/// Whether this device is a tablet; a phone when hostnamed cannot say.
pub async fn is_tablet() -> bool {
    match chassis().await {
        Ok(chassis) => chassis == "tablet",
        Err(error) => {
            eprintln!("sensors: reading the chassis from hostnamed: {error}");
            false
        }
    }
}

async fn chassis() -> zbus::Result<String> {
    let bus = Connection::system().await?;
    let props = PropertiesProxy::builder(&bus)
        .destination(NAME)?
        .path(PATH)?
        .build()
        .await?;
    let chassis = props.get(InterfaceName::try_from(NAME)?, "Chassis").await?;
    Ok(String::try_from(chassis)?)
}
