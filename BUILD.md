# Build LumoDent

LumoDent membutuhkan Rust 1.95 atau lebih baru. Dependensi diunduh dari crates.io pada build pertama, setelah itu `--offline` dapat dipakai. `Cargo.lock` mengunci versinya.

## Windows (x64 dan ARM64)

Workspace ini memakai toolchain `stable-x86_64-pc-windows-gnullvm` dengan [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) sebagai linker. `.cargo/config.toml` memanggil `x86_64-w64-mingw32-clang` dan `aarch64-w64-mingw32-clang`, jadi folder `bin` llvm-mingw harus ada di PATH (misalnya `winget install MartinStorsjo.LLVM-MinGW.UCRT`).

```powershell
$cargo = Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'
& $cargo +stable-x86_64-pc-windows-gnullvm test
& $cargo +stable-x86_64-pc-windows-gnullvm build --release
rustup target add aarch64-pc-windows-gnullvm --toolchain stable-x86_64-pc-windows-gnullvm
& $cargo +stable-x86_64-pc-windows-gnullvm build --release --target aarch64-pc-windows-gnullvm
```

Hasilnya `target/release/lumodent.exe` (x64) dan `target/aarch64-pc-windows-gnullvm/release/lumodent.exe` (ARM64). Toolchain MSVC dengan Visual Studio Build Tools juga dapat dipakai (`--target x86_64-pc-windows-msvc` atau `aarch64-pc-windows-msvc`). Aplikasi memakai `winsqlite3.dll` bawaan Windows 10/11.

## Linux dan macOS

Linux membutuhkan pustaka SQLite untuk build (`libsqlite3-dev` di Debian/Ubuntu). Saat berjalan, aplikasi membutuhkan `libsqlite3-0`, driver Vulkan atau OpenGL, `libxkbcommon`, serta X11 atau Wayland. Dialog berkas memakai xdg-desktop-portal. macOS memakai SQLite sistem.

```sh
cargo test
cargo build --release
```

Mesin pengembangan ini tidak memiliki Linux atau macOS (WSL tidak terpasang), sehingga kedua platform tersebut belum dibuild dan diuji di sini.

## Pengujian

`cargo test` menjalankan 23 tes inti dan 3 uji UI headless. Tes inti mencakup database pasien, preset tanggal, Study/Report dengan penghapusan berantai, dekoder BMP/PNG/JPEG, operasi citra, kurva tone, geometri zoom, format proyek laporan, tata letak halaman, encoder BMP, PDF, serta alur folder data di `workspace`. Uji UI (`src/ui/tests.rs`, `egui_kittest`) mengklik dan mengetik lewat pohon aksesibilitas: CRUD pasien dengan validasi, navigasi tanpa pasien, dan alur Viewer → Report (impor, rotasi, Send to Report, simpan, Export PDF, Print, muat ulang, hapus laporan dan pasien). Dialog berkas dan pembuka PDF diganti hook uji.

`cargo test --release render_screens -- --ignored` mengisi data demo dan merender setiap layar ke `target/screens` dengan wgpu, tanpa jendela.

## Hasil di mesin ini

Pada Windows 11 x64 dengan Rust 1.100.0 dan llvm-mingw 20261020: 23 tes inti dan 3 uji UI lulus, dan render layar berhasil. Build release x64 dan ARM64 berhasil dan hanya mengimpor DLL bawaan Windows (termasuk `winsqlite3.dll`, `dxgi.dll`, `uiautomationcore.dll`), tanpa runtime llvm-mingw. EXE x64 dijalankan dan membuka jendela LumoDent. EXE ARM64 belum dijalankan karena tidak ada perangkat ARM64.

| Target | Ukuran | SHA-256 |
|---|---:|---|
| x86_64-pc-windows-gnullvm | 14.101.504 byte | `A066EBC6B3347D0F2DC002FA68BBD2C5A08FE1F28E05E1B8F8AA2BF09971DF4D` |
| aarch64-pc-windows-gnullvm | 12.209.152 byte | `640640ADBC0D4101D63AFC09E1351FBBAC575EC8115FD552AA6289318195EB1A` |
