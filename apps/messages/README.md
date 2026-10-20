# Messages

SMS app for Denial phones, written with GPUI. It is still a UI sample: the
conversations come from `src/data.rs` and nothing talks to a modem yet.

## Dependencies

- `libraries/denial-look`: Denial's application look (palette, glass groups,
  user accent and opacity from `~/.config/denial/settings.json`).
- [denialwm/gpui-touch](https://github.com/denialwm/gpui-touch), branch
  `denial/touch-fling-end`: zed's GPUI with Wayland touch
  (zed-industries/zed#64079) plus Denial's touch fixes. Pinned by commit;
  push the fork before bumping `rev` here.

Denial sends touch only as `wl_touch`, so upstream GPUI cannot be used on
devices.

## Build

On the host:

```sh
cargo run --release
```

For the Ace 3, cross-built against its sysroot in
`/mnt/development/cross/sysroot`:

```sh
./build-aston.sh
```

Both write to `target/` at the denial-apps root.
