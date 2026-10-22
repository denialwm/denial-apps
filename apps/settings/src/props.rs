//! Reading D-Bus objects as their managers list them: object path, then
//! interface, then property.

use std::collections::{BTreeMap, HashMap};

use zbus::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

pub type Props = HashMap<String, OwnedValue>;
/// Interfaces by name, each with its properties.
pub type Interfaces = HashMap<String, Props>;
/// Objects by path, in path order.
pub type Objects = BTreeMap<String, Interfaces>;

/// Every object `service` manages, read with `GetManagedObjects` at `root`.
pub async fn objects(bus: &Connection, service: &str, root: &str) -> zbus::Result<Objects> {
    let reply = bus
        .call_method(
            Some(service),
            root,
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;
    let objects: HashMap<OwnedObjectPath, Interfaces> = reply.body().deserialize()?;
    Ok(objects
        .into_iter()
        .map(|(path, interfaces)| (path.to_string(), interfaces))
        .collect())
}

/// Property `name` as a `T`, if it is there and is one.
pub fn get<T>(props: Option<&Props>, name: &str) -> Option<T>
where
    T: TryFrom<Value<'static>>,
{
    let value = props?.get(name)?.try_clone().ok()?;
    T::try_from(Value::from(value)).ok()
}

/// An object path property as a string; "/" means none.
pub fn path(props: Option<&Props>, name: &str) -> Option<String> {
    get::<OwnedObjectPath>(props, name)
        .map(|path| path.to_string())
        .filter(|path| path != "/")
}

/// `value` as an `OwnedValue`, for settings sent back.
pub fn owned<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    OwnedValue::try_from(value.into()).expect("plain values hold no file descriptors")
}
