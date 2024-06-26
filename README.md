# Device-agnostic Cross-platform Tablet Library 🐙✨
[![Crates.io Version](https://img.shields.io/crates/v/octotablet?style=for-the-badge)](https://crates.io/crates/octotablet)
[![docs.rs](https://img.shields.io/docsrs/octotablet?style=for-the-badge)](https://docs.rs/octotablet/latest/octotablet/)

Cross-platform high-level tablet/pad/stylus library, reporting connected hardware features and providing easy-to-use
event-based access to user input.

## [Examples](examples/README.md)
See [the examples](examples/README.md) for features and usage with several windowing abstractions.
In particular, `eframe-viewer` provides a nice interface to debug your devices and view many of the capabilities of this crate.

## Platform Support
| Platform                             |      Support |
|--------------------------------------|-------------:|
| Linux/Wayland (`tablet_unstable_v2`) | Complete[^1] |
| Windows (Ink `RealTimeStylus`)       | Complete[^2] |
| Linux/X11 (`xinput`)                 |     I'll try |
| MacOS                                |  Help needed |
| IOS                                  |  Help needed |
| Android                              |  Help needed |
| Windows (`Winuser.h` Pointer API)    |  Not planned |
| Windows (`wintab`, proprietary)      |  Not planned |

[^1]: Compositor conformance to this protocol is hit or miss and some features may not work (to be expected from an unstable protocol I guess!)
[^2]: Only Tablets and Tools - Pads and associated hardware are not exposed by the Ink API. The status of pad hardware on windows is dire, often reported as emulated mouse/keyboard events!

## Device Support
So far, tested on:
* *Wacom Cintiq 16* \[DTK-1660\]
* *Wacom Intuos (S)* \[CTL-4100\]
* *Wacom Intuos Pro small* \[PTH-451\]
* *Wacom Pro Pen 2*
* *Wacom Pro Pen 2k*
* *XP-Pen Deco-01*

## Documenting
By default, documentation contains the current platform's capabilities only (ie, building docs on windows will omit everything wayland-related).
Run `rustdoc` with the `docsrs` cfg set in order to generate documentation for all platforms regardless of host platform:
```bash
RUSTFLAGS="--cfg docsrs" cargo doc
```
This is still restricted by enabled features.
