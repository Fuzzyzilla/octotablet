# Example apps
Run using the standard `cargo` examples functionality, eg:
```bash
cargo run --example eframe-viewer
```

## `winit`
Demos integration with a basic empty winit window. Dumps collected information to stdout.

## `eframe-viewer`
Demos integration with egui for exploring much of the data this crate provides, including
listing connected tablet/pad/stylus hardware with their capabilities. Also includes a test
area where you can play with and visualize the distance/tilt/pressure capabilites of your tablet.
