# LumoDent

Intraoral X-ray workstation written in Rust, by **illustricious**. LumoDent is a rewrite of the NanoPix 1.1.1.9 workstation software: its behaviour (patient database, image viewer, reports) was mapped from that application, while the name, logo, icons and code are LumoDent's own.

It runs on Windows (x64 and ARM64) and Linux, from one code base built with [egui](https://github.com/emilk/egui).

## Features

- **Patient**: search by chart number, name or phone, quick search for recently acquired or printed patients, nine date filters, create, edit and delete patients.
- **Viewer**: import JPG, PNG, BMP and DICOM radiographs, export PNG and DICOM, thumbnails, rotate, flip, invert, brightness and contrast, pan and zoom.
- **Report**: send images from the viewer, paper size and orientation, clinic header, notes, save and reopen reports, export to PDF and print.
- **Acquisition**: not available yet (sensor support is still being mapped).

## Install

Download an installer from the [Releases](../../releases) page:

| System | File |
|---|---|
| Windows 10/11 x64 | `LumoDent-<version>-windows-x64.msi` |
| Windows 11 ARM64 | `LumoDent-<version>-windows-arm64.msi` |
| Debian/Ubuntu x86-64 | `lumodent_<version>_amd64.deb` |
| Debian/Ubuntu ARM64 | `lumodent_<version>_arm64.deb` |

Data is stored in `%LOCALAPPDATA%\LumoDent` on Windows and `~/.local/share/lumodent` on Linux. Set `LUMODENT_DATA` to use another folder.

## Build from source

```sh
cargo test
cargo build --release
```

Linux needs `libsqlite3-dev`. See [BUILD.md](BUILD.md) for the Windows toolchain and packaging details. Project notes (in Indonesian): [PROGRESS.md](PROGRESS.md), [DOMAIN.md](DOMAIN.md), [UI.md](UI.md).

LumoDent is not a certified medical device.
