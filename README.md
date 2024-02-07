# Unnamed Cross-Platform Tablet Library

Work in progress high-level tablet/stylus input library with aspirations
to be an easy-to-use cross-platform API without losing features to abstraction.

## Platform Support
| Platform                             |       Support |
|--------------------------------------|--------------:|
| Linux/Wayland (`tablet_unstable_v2`) |      Full[^1] |
| Windows (Ink `RealTimeStylus`)       |  In progress! |
| Linux/X11 (`xinput`)                 |      I'll try |
| Windows (`Winuser.h` Pointer API)    | I'll consider |
| MacOS                                |   Help needed |
| IOS                                  |   Help needed |
| Android                              |   Help needed |
| Windows (`wintab`, proprietary)      |   Not planned |

[^1]: Compositor support/conformance for this protocol is hit or miss and some features may not work (to be expected from an unstable API I guess!).
## Device Support
So far, tested on:
* *Wacom Cintiq 16* \[DTK-1660\]
* *Wacom Intuos (S)* \[CTL-4100\]
* *Wacom Intuos Pro small* \[PTH-451\]
* *Wacom Pro Pen 2*
* *Wacom Pro Pen 2k*
* *XP-Pen Deco-01*

## Documenting
Run `rustdoc` with the `docsrs` cfg set in order to generate documentation for all platforms regardless of host platform:
```bash
RUSTFLAGS="--cfg docsrs" cargo doc
```
This is still restricted to enabled features.
