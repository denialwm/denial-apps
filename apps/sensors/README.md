# Sensors

Shows what the phone's sensors read, written with GPUI. The readings come
from steinerd, Phonewave's sensor service (`org.steinerd` on the system bus,
`phonewave/services/sensors/steinerd`), which today runs on the RedMagic
11 Pro (nx809j).

It is drawn to Denial's mobile materials, Islands
(denial/docs/MOBILE_APP_MATERIALS.md): the title in a glass group at the top
and one island per sensor steinerd offers, each named inside it.

- **Orientation**: the phone drawn as it is held, its earpiece and camera
  marking the top edge, turning the short way round when the edge pointing
  up changes; the edge's name, and portrait or landscape. Lying flat keeps
  the last one, as steinerd does.
- **Ambient light**: the lux, what light that is (dark, dim, indoors,
  bright indoors, daylight, direct sun), a meter on a log scale from 1 lx to
  direct sun at 100,000 lx, and the last minute as a graph. The graph's
  scale is logarithmic too, fitted to what the minute saw and a decade tall
  at least; each reading holds until the next, since the sensor reports only
  changes.
- **Proximity**: rings around a dot, which take the accent and grow while
  something covers the sensor.

A sensor gives readings only while someone claims it. Sensors claims
orientation, light and proximity while its window shows and releases them
while it is hidden; steinerd also drops a client's claims when it leaves the
bus. While steinerd is not running, one island says so, and Sensors follows
it again as soon as it starts. Back and Escape close the window.

One Sensors runs at a time: it owns `org.denialwm.Sensors` on the session
bus, and a second `sensors` raises its window and exits.

## Dependencies

- `libraries/denial-look` and `libraries/denial-look-gpui`.
- The local gpui-touch checkout, as in Messages (see its README).

## Build

On the host:

```sh
cargo run --release
```

For the phones, cross-built against the sysroot in
`/mnt/development/denial-os/cross/sysroot`:

```sh
./build-aarch64.sh
```

Both write to `target/` at the denial-apps root. On the phone, the binary
goes in `~/.local/bin/sensors`, with the `.desktop` file and icon as the
denial-apps README says.

`cargo test -- --ignored --nocapture reads_steinerd`, run on a phone,
follows steinerd for 5 s with the sensors claimed and prints what it
reports.
