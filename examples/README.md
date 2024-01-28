# Example apps
Run using the standard `cargo` examples functionality, eg:
```bash
cargo run --example eframe-viewer
```

## `winit-paint`
Demos integration with a `winit` window acting as a very simplistic sketchpad via `tiny-skia`, as an example of this crate's
usage in drawing apps throug the `events` API. Since this does rendering on the CPU for simplicity, running in `--release`
mode is necessary for usable framerates!

![Drawing with a sheep and the text "Hello World~!"](images/winit-paint.png)

## `eframe-viewer`
Demos integration with `eframe` for exploring much of the data this crate provides, including listing connected tablet/pad/stylus
hardware with their capabilities. Also includes a test area where you can play with and visualize the distance/tilt/pressure
capabilities of your tablet through the `summary` API.
