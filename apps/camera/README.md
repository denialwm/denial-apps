# Camera

Camera app for Denial phones, written with GPUI. It drives the sensors
itself, without libcamera: it routes one sensor through qcom-camss to memory
as packed 10-bit raw (`src/device.rs`), and everything after that is ours.
Values are for the Ace 3 (aston): the IMX890 main camera and the S5K3P9
front camera (`src/sensor.rs`).

It keeps to Denial's mobile materials
(denial/docs/MOBILE_APP_MATERIALS.md) as an opaque window: a camera is judged
on its picture, so it is always dark, on black, whatever the appearance, with
raised islands for its controls and glass only over its own picture. The
preview is centred in the sensor's upright 3:4. Above it, a pill holds the
timer (off, 3 s, 10 s; the shutter shows a stop while it counts) and the
rule-of-thirds grid. On the preview, 1x and 2x: 2x crops the middle of the
frame in the preview, its metering and the photo. Under it: the last photo,
with a turning arc while photos develop, the shutter and the camera switch.
A tap on the preview focuses there (main camera).

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

## Preview (`src/isp.rs`)

Every frame becomes a 768x1024 upright preview straight from the raw
samples: one 2x2 cell per pixel, then lens shading, white balance, the
colour matrix with the look, and the tone. The same pass gathers the
statistics:

- **Lens shading:** the stock tuning's gain maps of each module, 17x13 per
  colour (`src/shading.rs`, generated from the facts exports), for three
  colour temperatures, interpolated in mired. The IMX890's corners get
  about 5x the light of the centre. The maps are applied fully, as the
  RedMagic does in a dim room, until the scene needs digital gain; then
  they dim, to the power 0.4 at 8x digital gain, so the corners do not show
  the noise they lift, as the stock tuning dims them in low light.
- **Auto exposure:** meters the mean linear luminance at 18% grey, and lets
  at most 4% of the frame sit above 90% of full scale. It moves by the
  square root of what it asks, at most 1.6x a step, with an 8% dead band,
  every 3 frames. Longest time first (33 ms), then gain, up to 16x, then
  digital gain, up to 8x. Digital gain lifts shadows and mid tones and
  leaves the sensor's clipping where it is, so lights in a dark room keep
  their colour.
- **White balance:** a 16x12 grid of zones of unclipped colour sums. Only
  zones within 0.06 of the stock illuminant locus count, and their grey is
  projected onto the locus. That gives the light's colour temperature. A
  light cooler than 5500 K is made neutral; a warmer one is moved 40% of
  the way toward 5500 K and stays a little warm (a first guess). Gains and
  the stock matrix come from that temperature: the QTech module's for the main camera, the Sunny module's
  for the front one.
- **Look and tone:** fitted to the RedMagic 11 Pro+ on a normally exposed
  scene, a video on a monitor (2026-10-22), block by block. After the
  matrix, hue 4 degrees toward blue. Then each channel goes through a
  filmic S curve: deep shadows, contrasty mid tones, highlights rolling off,
  and light past clipping reaching white. Per channel, as a camera's gamma,
  a saturated colour stays saturated and a bright one turns toward white
  as its strongest channel tops out. A cell near clipping goes to
  white only when it is near grey (a light source); a saturated colour that
  clips, such as a lit blue wall, keeps its colour.
- **Focus (main camera):** a contrast sweep. Coarse over the whole travel,
  then fine around the best position. It runs when the camera opens and on
  every tap.

## Photo (`src/photo.rs`)

The shutter takes a bracketed raw burst around the exposure auto exposure
chose:

| Group | Exposure | Frames |
|---|---|---|
| Long (shadows) | 2x | 8 |
| Short | 1/4 | 4 |
| Shortest (light sources) | 1/16 | 4 |

That takes about 0.85 s. The burst is then developed by the aquarium
"money shot" method (~/.cache/phonewave/aston/camera-pipeline-20261021):

1. Frames are aligned to the short group's first frame. The shift is a
   global one, in whole 2x2 cells, found coarse to fine on log radiance.
2. Groups are merged on the raw mosaic in radiance, weighted by exposure x
   frames, leaving out cells that clipped. What clips even in the shortest
   group is held white.
3. Lens shading, as in the preview, then demosaicing (Malvar-He-Cutler).
4. White balance, the digital gain, the matrix and the tone of the
   preview at the shutter, with radiance taken at the preview's exposure,
   so the photo looks like the preview with less noise and more detail.
   The image is turned upright.

Photos are JPEGs at the sensor's full output, 3072x4096 from the main
camera and 1728x2304 from the front one, in `~/Pictures/Camera/IMG_<date>_<time>.jpg`.
A main-camera photo takes about 4.8 s and 1.5 GB on the phone, a front one
1.4 s. The front camera's photos are mirrored, as its
preview is.

## Raw frames for tuning

While `~/Pictures/Camera/raw` exists, every photo also keeps the raw frame
the preview showed when the shutter was pressed: `<photo name>.raw`, packed
RAW10 as the sensor sends it, and `<photo name>.txt` with the sensor, size,
stride, Bayer order, black level, exposure, white balance gains, colour
temperature and matrix. Remove the folder to stop.

## Not yet

- Alignment is a global shift: no tile alignment or ghost removal for
  moving subjects.
- No continuous autofocus.
- 50 MP is missing. The main camera's full mode (8192x6144) is not used:
  the imx890 driver has only the 2x2-binned 4096x3072 mode. The full mode
  needs:
  - The stock mode 0 in the driver. Its registers and timing are in
    camera-facts-20261021: 24 fps, 2.9 Gsps per C-PHY trio, twice today's
    rate, so the CSIPHY needs settings for it.
  - The IMX890 is Quad Bayer: full readout needs remosaicing to a normal
    Bayer mosaic, on the sensor or in our pipeline, and possibly the
    module's own sensitivity calibration from its EEPROM.

## Build

On the host, `cargo run --release` builds it, but there is no camera to
open. For the Ace 3, cross-built against its sysroot in
`/mnt/development/denial-os/cross/sysroot`:

```sh
./build-aston.sh
```

Both write to `target/` at the denial-apps root. On the phone it runs as
`~/.local/bin/camera`, with `org.denialwm.Camera.desktop` in
`~/.local/share/applications` and its icon as the root README says. Its user must be in the `video` group.
