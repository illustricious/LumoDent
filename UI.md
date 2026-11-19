# Antarmuka LumoDent

[Referensi startup asli](reference-startup.jpg) memperlihatkan menu File/Tools/Help, panel pencarian di kiri, navigasi Patient/Acquisition/Viewer/Report, toolbar pasien, pilihan tanggal, kartu demografi, daftar pasien, panel studi dan dialog REGISTER PRODUCT. LumoDent mengikuti susunan dan label tersebut, dengan logo, ikon dan warna milik sendiri.

| Layar | Isi | Kode |
|---|---|---|
| Kerangka | Title bar sendiri tanpa border sistem: menu, judul, tombol Minimize/Maximize/Close dalam satu baris; seret untuk memindah, klik ganda untuk maximize, tepi jendela untuk resize. Menu File (New Patient, Import Image, Export PNG/DICOM/PDF, Print, Exit), View (Patient/Viewer/Report, Full Screen), Tools (Theme, Open Data Folder, Open Patient Image Folder), Help (Keyboard Shortcuts, About). Nama ditampilkan nama depan dulu. Logo LumoDent, tab navigasi dengan ikon, nama pasien aktif | `src/ui/mod.rs` |
| Patient | SEARCH dan QUICK SEARCH (Recently Acquired/Printed), toolbar New/Edit/Delete Patient dan Clear List, filter tanggal sembilan preset, kartu ringkasan, Show Detail, daftar hasil (klik untuk memilih, klik ganda untuk mengubah), formulir NEW/EDIT PATIENT sebelas kolom | `src/ui/patient.rs` |
| Acquisition | Belum tersedia; tab menampilkan pesan | — |
| Viewer | Daftar IMAGES dengan thumbnail, Import Image dan Delete Image, Rotate, Flip Horizontal, Flip Vertical, Invert, Fit, Send to Report, kanvas dengan pan dan zoom, Zoom In/Out, slider Brightness dan Contrast | `src/ui/viewer.rs` |
| Report | REPORT LIST, New Report dan Delete Report, Paper Size, Orientation, Header Show, Footer Show, Save Report, Export PDF, Print, CLINIC INFORMATION, Note, REPORT IMAGES dengan thumbnail dan Remove Image, pratinjau halaman | `src/ui/report.rs` |

Tema: Tools → Theme → Dark/Light, disimpan di `theme.txt`. Bahasa: Tools → Language → English/Bahasa Indonesia, disimpan di `language.txt` (terjemahan di `src/ui/i18n.rs`; teks di halaman laporan/PDF masih bahasa Inggris). Viewer juga punya Filters (Auto Level, Enhance Detail, Sharpen, Smooth, Reduce Noise, Normalize dari `src/processing.rs`), Measure (penggaris; mm bila berkas DICOM memuat Pixel Spacing, selain itu px), Rotate Left, Export Image (PNG sesuai tampilan), Export DICOM, import `.dcm`, Reset brightness/contrast, dan shortcut keyboard: `+`/`-` zoom, `R`/`L` rotate, `H`/`V` flip, `I` invert, `F` atau `0` fit, panah atas/bawah pindah gambar.

Semua ikon digambar sebagai vektor di `src/ui/icons.rs`, termasuk logo (lingkaran dengan bulan sabit) dan ikon jendela di `src/main.rs`. Ukuran jendela minimum 1024 × 700.

Belum dikerjakan: registrasi produk, menu Tools lengkap, panel studi di layar Patient, modul Acquisition, lokalisasi dan kesetaraan visual per piksel dengan aplikasi referensi.

Tangkapan layar dirender tanpa jendela: `cargo test --release render_screens -- --ignored` menulis `target/screens/*.png` dengan data demo. Lihat [tracker](PROGRESS.md) dan [build serta pengujian](BUILD.md).
