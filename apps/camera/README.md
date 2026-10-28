# Camera

Camera app for Denial phones, written with GPUI. The cameras are braun's,
Phonewave's camera service (phonewave/services/camera/braun): it finds
them, streams them, runs exposure, white balance and focus, renders the
preview and develops the photos. The app picks the camera it faces (the
main back camera, the front one), shows braun's previews, and names and
keeps the photos.

It keeps to Denial's mobile materials
(denial/docs/MOBILE_APP_MATERIALS.md) as an opaque window: a camera is judged
on its picture, so it is always dark, on black, whatever the appearance, with
raised islands for its controls and glass only over its own picture. The
preview is centred in the camera's upright aspect. Above it, a pill holds the
timer (off, 3 s, 10 s; the shutter shows a stop while it counts) and the
rule-of-thirds grid. On the preview, 1x and 2x: 2x crops the middle of the
frame in the preview, its metering and the photo. Under it: the last photo,
with a turning arc while photos develop, the shutter and the camera switch.
A tap on the preview focuses there, on a camera that can focus.

The gallery (`src/camera/gallery.rs`, `src/library.rs`) shows the photos one
by one: swipe between them, tap to hide the bars, double tap to zoom in
around the tap (2.5x, from the whole photo decoded for it) and out again;
a zoomed photo pans freely and keeps going after a fling. Its bars and
panels are smoke over the backdrop blur. The top bar has the day,
time and place, the bottom bar All photos, Info (date, size, file, and the
exposure when the raw notes are kept) and Delete, with a confirmation. The
JPEG and its thumbnail go; a kept raw stays. All photos is a grid by day.
Thumbnails are cached in `~/.cache/denial-camera/thumbnails`; photos are
decoded screen-sized on a thread of their own, the shown one and its
neighbours only. The camera is closed while the gallery is open.

While Denial suspends the window (minimized, or not in front), the camera is
closed, and it opens again when the window is shown.

## Photos

Photos are JPEGs at the sensor's full output in
`~/Pictures/Camera/IMG_<date>_<time>.jpg`. braun writes each into
`<name>.jpg.partial`, which the app renames once braun says it is whole.
The arc on the last photo turns while photos develop.

## Raw frames for tuning

While `~/Pictures/Camera/raw` exists, every photo also keeps the raw frame
the preview showed when the shutter was pressed, as braun keeps it:
`<photo name>.raw`, packed RAW10 as the sensor sends it, and
`<photo name>.txt` with the sensor, size, stride, Bayer order, black level,
exposure, white balance gains, colour temperature and matrix. Remove the
folder to stop.

## Build

On the host, `cargo run --release` builds it; it opens a camera where braun
runs. For the Ace 3, cross-built against its sysroot in
`/mnt/development/denial-os/cross/sysroot`:

```sh
./build-aston.sh
```

Both write to `target/` at the denial-apps root. On the phone it runs as
`~/.local/bin/camera`, with `org.denialwm.Camera.desktop` in
`~/.local/share/applications` and its icon as the root README says, next
to braun.
