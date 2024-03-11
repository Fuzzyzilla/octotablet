# Device-agnostic Cross-platform Tablet Library 🐙✨

Work in progress high-level tablet/pad/stylus library, reporting connected hardware features and providing easy-to-use
event-based access to user input.

## Platform Support
| Platform                             |                                                                     Support |
|--------------------------------------|----------------------------------------------------------------------------:|
| Linux/Wayland (`tablet_unstable_v2`) |                                                             In progress[^1] |
| Windows (Ink `RealTimeStylus`)       | [Stalled](https://github.com/Fuzzyzilla/octotablet/pull/1#issue-2177999569) |
| Linux/X11 (`xinput`)                 |                                                                    I'll try |
| Windows (`Winuser.h` Pointer API)    |                                                                     Planned |
| Windows (`wintab`, proprietary)      |                                                                     Maybe?? |
| MacOS                                |                                                                 Help needed |
| IOS                                  |                                                                 Help needed |
| Android                              |                                                                 Help needed |

[^1]: Compositor support/conformance for this protocol is hit or miss and some features may not work (to be expected from an unstable protocol I guess!)

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
