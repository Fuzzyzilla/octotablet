# Example apps
Run using the standard `cargo` examples functionality, eg:
```bash
cargo run --example eframe-viewer
```

## `winit-paint`
Demos integration with a `winit` window acting as a very simplistic sketchpad via `tiny-skia`, as an example of this crate's
usage in drawing apps throug the `events` API. Since this does rendering on the CPU for simplicity, running in `--release`
mode is necessary for usable framerates! However, if you don't do this - notice how the lines remain smooth even with abysmal
frame lag - the events api will never coalesce events, leaving them as detailed as possible.

![Drawing with a sheep and the text "Hello World~!"](images/winit-paint.png)

## `eframe-viewer`
Demos integration with `eframe` for exploring the data this crate provides, including listing connected tablet/pad/stylus
hardware with their capabilities. Also includes a test area where you can play with and visualize the distance/tilt/pressure
capabilities of your tablet and observe the raw event stream.

## `sdl2`
Demos integration with the `sdl2` crate, and does little more than that. Currently, `sdl2` requires the use of `raw-window-handle = 0.5.0`
whereas `octotablet` requires the use of `raw-window-handle = 0.6.0`, this demo shows how to bridge between the two. In the future,
this should be fixed.
Octotablet currently requires wayland, which can be accessed via the environment variable `SDL_VIDEODRIVER`.
```bash
SDL_VIDEODRIVER=wayland cargo run --example sdl2
```
