# LumoDent — progres rewrite Rust

LumoDent adalah aplikasi workstation radiografi intraoral yang ditulis ulang dalam Rust. Developer: illustricious. Perilaku aplikasi dipetakan dari NanoPix 1.1.1.9 sebagai aplikasi referensi. Nama dan logo LumoDent tidak memakai merek dagang NanoPix atau Eighteeth. Seluruh logo dan ikon digambar sebagai vektor dalam kode (`src/ui/icons.rs`); tidak ada bitmap dari paket asli. Antarmuka memakai egui sehingga kode yang sama berjalan di Windows (x64 dan ARM64), Linux dan macOS.

## Titik awal

Milestone pertama adalah alur Patient di Windows. Alur ini melintasi navigasi, formulir, penyimpanan lokal dan pencarian sehingga dapat diuji dari UI sampai ke data tanpa sensor. Aplikasi Rust memiliki event loop, kontrol dan penyimpanan sendiri. Versi awal memakai Win32 dan bitmap dari paket asli; keduanya sudah diganti (lihat bagian Antarmuka lintas platform).

| Hasil yang dapat diperiksa | Verifikasi |
|---|---|
| 23 rutin dipetakan | 13 rutin di executable dan 10 metode di CoreLibrary.dll. Sebagian masih pada tingkat pemanggilan API, khususnya koreksi citra dan DICOM |
| Build Rust | Windows x64 dan ARM64 dari mesin ini. Linux dan macOS memakai kode yang sama tetapi belum dibuild di sini |
| Data pasien | Tes Cargo pasien lulus. Uji UI headless mencakup tambah, pencarian cocok/kosong, ubah, buka ulang, batal hapus dan konfirmasi hapus |
| Dua pencarian aktivitas dan sembilan preset tanggal | Kueri diuji dengan studi/laporan sintetis. Batas kalender pilihan rewrite perlu dibandingkan dengan produk asli |
| Studi dan Viewer | Tabel Study/Report dengan skema asli, impor BMP/PNG/JPEG, daftar citra per pasien, rotasi, flip, invert, zoom, pan, brightness/contrast. Tes inti dan uji UI headless lulus |
| Kesamaan tampilan | [Referensi startup asli](reference-startup.jpg) tersedia. Kesetaraan seluruh layar masih terbuka |

## Peta fungsi executable

Alamat merupakan VA pada image base `0x400000` untuk NanoPix.exe x86 dengan SHA-256 `1125f89f6c7b31a4c7674dc225ed89dfcba38a6a97072306c16d1346954af739`. Nama peran di bawah merupakan interpretasi dari pseudocode Ghidra. Panggilan dan literal adalah pengamatan statis. Simpanan REA memuat 23 rekaman, termasuk pseudocode, xref, batas prosedur dan profil analisis. Awalan ID di tabel dapat dicari dalam [bundle bukti](rea-evidence.json).

| VA | Fungsi yang dipetakan | Bukti dalam badan fungsi | Cakupan Rust |
|---|---|---|---|
| 0x4896c0 | Startup dan loop jendela | Menyiapkan font/ikon, membuat jendela NanoPix 1024 × 768, mengatur batas ukuran, memanggil swap buffers sampai jendela ditutup | Jendela utama egui (`src/main.rs`, `src/ui/mod.rs`) |
| 0x4eb520 | Pembuatan jendela tambahan | Membuat window berjudul sementara “No Title Yet”, memasang callback posisi, ukuran, keyboard, mouse | Belum |
| 0x563550 | Memuat daftar pasien | Memanggil DataAccessor::GetPatients pada 0x5638ec lalu menyerahkan hasil ke rutin lain | Pencarian dan pemuatan lokal |
| 0x560a80 | Menambah pasien | AddPatient pada 0x560d1c, hasil nol memicu pemuatan ulang daftar | Tambah dan perbarui daftar |
| 0x563e90 | Mengubah pasien | UpdatePatient pada 0x563f69, jalur berhasil dan pesan kegagalan database terpisah | Ubah data lokal |
| 0x5613e0 | Menghapus pasien | DeletePatient pada 0x561606, juga ada jalur kegagalan penghapusan berkas citra | Hapus baris pasien, Study/Report ikut terhapus lewat ON DELETE CASCADE, folder citra pasien dihapus dengan pesan kegagalan sendiri |
| 0x53a090 | Memuat produk | DataAccessor::GetProducts pada 0x53a0b5 | Belum |
| 0x5200f0 | Menyimpan berkas DICOM | DicomProcessor::SaveDicomFile pada 0x520281 | Selesai (`src/dicom.rs`, Export DICOM) |
| 0x50e020 | Membaca ukuran citra sensor | Sensor_GetImageSize pada 0x50e032 | Belum |
| 0x4f6490 | Jalur koreksi citra | Sensor_CorrectImage pada 0x4f6714 | Belum. Algoritma piksel belum dipetakan |
| 0x580ff0 | Panel pencarian dan pengaturan layout | Referensi Child_Search, Child_Recent, teeth_layout, viewer_layout, template, Child_Table | Bagian pencarian pasien |
| 0x58d2d0 | Pengaturan tampilan | Kelompok fullscreen, layout, magnifier, warna capture, jarak dan warna grid | Belum |
| 0x5aaa00 | Daftar laporan | Merujuk ID kontrol ##ReportList | Belum |

Enam baris pertama berasal dari batch `ev_793b9224`, empat berikutnya dari `ev_ba7c8460`, dan tiga terakhir dari `ev_39cf3168`. Inventaris `ev_8564a21d` mencatat 25.801 prosedur dan 4.320 string. Inventaris mencakup thunk serta kode pustaka, sehingga jumlahnya tidak mengukur persentase fitur yang selesai.

## Peta lapisan data dan antarmuka

- [Peta CoreLibrary.dll](DOMAIN.md) menelusuri sepuluh fungsi tambahan dari nama ekspor ke instruksi yang mereferensikan SQL. Alamat di dokumen tersebut berupa RVA DLL.
- [Pemetaan UI](UI.md) mendokumentasikan layar referensi dan cakupan interaksi.
- CorradeIcons.dll menyimpan tabel 230 nama PNG dan rentang datanya. Rewrite tidak memakainya: label tombol mengikuti teks UI referensi, sedangkan ikon digambar ulang sebagai vektor.
- FinCloud adalah proses Electron terpisah. Di `FinCloud/resources/app.asar`, manifest menunjuk `dist-electron/main.js`. Entry tersebut membuat BrowserWindow, memuat `../dist/index.html` pada jalur rilis, dan meneruskan argumen proses ke renderer saat menerima pesan `treeUrl`. Koneksi layanan, login dan sinkronisasi belum diverifikasi saat berjalan.
- [Cara build dan pengujian](BUILD.md) menjelaskan toolchain serta lokasi executable.

## Urutan pekerjaan berikut

| Tahap | Kriteria selesai | Status |
|---|---|---|
| 1. Patient | Layar pasien, CRUD, persistensi, pencarian dan verifikasi terhadap aplikasi asli | Implementasi awal tersedia. Lihat batas cakupan UI dan data di atas |
| 2. Studi dan Viewer | Dari pasien terpilih, buka studi contoh dan cocokkan thumbnail, orientasi, zoom serta pan | Implementasi awal tersedia untuk BMP, PNG dan JPEG. DICOM, thumbnail bergambar, penyimpanan hasil edit dan pembandingan dengan aplikasi asli masih terbuka |
| 3. Report | Buat laporan dari studi, simpan, buka kembali dan bandingkan halaman keluaran | Implementasi awal tersedia: Send to Report, pengaturan kertas, header/footer, info klinik, catatan, simpan/muat ulang/hapus dan cetak. Pembandingan halaman dengan aplikasi referensi masih terbuka |
| 4. Acquisition | Cocokkan siklus perangkat, ukuran citra dan keluaran koreksi terhadap rekaman uji serta sensor yang sesuai | Dua titik integrasi native telah dipetakan |
| 5. Fungsi lintas modul | Verifikasi pengaturan, interoperabilitas database lama, registrasi, FinCloud, launcher, pembaruan, lokalisasi dan distribusi | Terbuka |

## Tahap 2: Studi dan Viewer

Literal SQL di CoreLibrary.dll memberi skema lengkap. Study berisi `pk`, `patient_id` (FK ke `Patient(id)` dengan `ON UPDATE CASCADE ON DELETE CASCADE`), `accession_number` (default 0), `sop_instance_uid`, `patient_age`, `modality`, `body_part_examined`, `image_name`, `image_type`, `description`, `datetime`, lalu dua timestamp. Report berisi `pk`, `patient_id`, `project_name` dan dua timestamp. Literal Report hanya memuat `project_name` karena string kolom bersama digabung oleh compiler, sehingga FK `patient_id` pada Report diasumsikan sama dengan Study.

| Bagian | Bukti asli | Implementasi Rust |
|---|---|---|
| Data Study | INSERT, UPDATE (nama, jenis, deskripsi), DELETE per PK, SELECT per modalitas, pasien dan rentang waktu | `src/studies.rs`. Urutan PK dipilih karena kueri asli tidak memiliki ORDER BY |
| Data Report | INSERT, UPDATE hanya `updated_datetime`, DELETE, SELECT per pasien dan rentang waktu | `src/studies.rs`. Belum ada UI |
| Modalitas | Literal `IO` di NanoPix.exe, parameter `ModalityType` di GetStudies | Viewer memakai `IO` |
| Nama berkas | Pola `%s/%s_%08X` di NanoPix.exe | `images/<chart no.>/<chart no.>_<8 hex>.<ekstensi asli>` di folder data. Byte berkas disalin apa adanya dan `image_type` mencatat ekstensinya. Arti nilai heks asli belum diketahui, rewrite memakai bit waktu |
| Format impor | Filter `Image Files (.jpg .jpeg .png .bmp .dcm)` | BMP (1/4/8/16/24/32 bit) dengan dekoder Rust, PNG dan JPEG melalui crate `image` (Rust murni, sama di semua OS). Orientasi EXIF JPEG belum diterapkan. DICOM masih terbuka |
| Operasi citra | Ekspor `RotateImage`, `FlipY`, `Invert`, `BrightnessAndContrast`, label `Flip Horizontal`/`Flip Vertical` | `src/image.rs`. Rumus piksel adalah pilihan rewrite. Edit hanya pada tampilan, tidak disimpan |
| Kanvas | Layout `viewer_layout`, kontrol zoom | `src/ui/viewer.rs`: tekstur GPU, drag untuk pan, roda mouse untuk zoom di titik kursor, tombol zoom dan Fit, thumbnail di daftar citra |

SOP Instance UID dibuat di bawah akar `2.25.` dari waktu dan PID. Ini belum UUID penuh dan perlu diganti saat DICOM diimplementasikan. `patient_age` dibiarkan kosong karena format tanggal lahir di formulir masih bebas.

Uji UI `ui::tests::viewer_send_to_report_save_export_print_and_delete` mengimpor PNG dan JPEG, menolak berkas bukan citra, memutar, membalik warna dan mengirim citra ke laporan. Uji `workspace::tests` memeriksa penamaan berkas, penghapusan studi dan penghapusan folder pasien.

## Tahap 3: Report

String UI aplikasi referensi menetapkan isi layar Report: `REPORT LIST`, `New Report`, `Save Report`, `Load Report`, `Print`, `Paper Size`, `Paper Orientation` (`Portrait`/`Landscape`), `Header Show`, `Footer Show`, `Clinic Information` (`Clinic Name`, `Clinic Address`, `Clinic Phone`), `Note`, serta `Send to Report` dari Viewer. Pesan seperti `Unable to save an empty report.`, `Save report successful, you can reload the report and edit it later.` dan `Delete report error, remove report file failed` dipakai ulang apa adanya. report.dll asli berbasis MFC dengan `PrintDlgW` dan print preview.

| Bagian | Implementasi Rust |
|---|---|
| Model laporan | `src/reports.rs`: kertas A4, A5, Letter, B5 (JIS), orientasi, header/footer, catatan, daftar citra. Format proyek `report.txt` milik rewrite dengan escape baris baru dan penolakan nama berkas berisi path |
| Tata letak halaman | Blok header klinik, baris pasien, grid citra yang memilih jumlah kolom paling dekat ke rasio 4:3, catatan, footer. Margin 12 mm |
| Penyimpanan | `reports/<chart no.>/<chart no.>_<waktu>/` berisi `report.txt` dan `img_NN.bmp`. Baris `Report` memakai `project_name` = nama folder. Simpan ulang memperbarui `updated_datetime` dan menulis ulang citra |
| Info klinik | `clinic.txt` di folder data, dipakai di semua laporan |
| Layar | `src/ui/report.rs`: daftar laporan, toolbar, panel klinik/catatan/citra dengan thumbnail, pratinjau halaman langsung |
| PDF dan cetak | `src/pdf.rs` menulis PDF satu halaman (Helvetica bawaan, citra RGB terkompresi Flate) dengan tata letak yang sama dengan pratinjau. `Export PDF` menyimpan lewat dialog Save, `Print` membuka PDF di penampil bawaan sistem untuk dicetak. Dialog cetak Win32 versi sebelumnya sudah diganti karena khusus Windows |
| Hapus pasien | Folder citra dan laporan pasien ikut dihapus, draf laporan pasien dibuang |

`Send to Report` mengambil citra seperti yang tampil di Viewer, termasuk rotasi, flip, invert dan brightness/contrast. Aplikasi referensi menyimpan JPG (`Send to report error, save jpg file failed`). Rewrite menyimpan BMP 24 bit supaya tidak menambah encoder.

Uji UI headless menyimpan laporan, mengekspor PDF, mencetak ke penampil (hook uji), membuka ulang laporan dari daftar, menanyakan konfirmasi sebelum membuang perubahan, menghapus laporan dan menghapus pasien. Uji `pdf::tests` memvalidasi xref, ukuran halaman, teks dan jumlah citra.

## Cakupan fungsi

Inventaris 25.801 prosedur di executable referensi didominasi pustaka (ImGui, spdlog, crashpad, Boost, CRT). Persentase dari angka itu tidak bermakna. Ukuran yang dipakai adalah metode kelas aplikasi yang diimpor dari DLL inti:

| Kelas | Peran | Sudah di Rust | Total |
|---|---|---:|---:|
| DataAccessor | Database lokal | 34 (semua: Patient, Recent, Study, Report, Analysis, Create/CheckMainDB di `patients.rs`; Product, User, CloudUser, Create/CheckUserDB di `users.rs`) | 34 |
| ImageProcessing | Olah citra | 17 (semua di `src/processing.rs`; 10 algoritma dipetakan dari biner, lihat bagian ImageProcessing) | 17 |
| DicomProcessor | DICOM | 6 (semua di `src/dicom.rs`: IsDicom → `is_dicom`, LoadFile/GetDicomImage → `parse`, SaveDicomFile → `write`, GetUid → `uid(UidKind)`, GetCurrentDateTime → `current_datetime`, GetInternal/FullDatasetMap → `Dicom::elements`) | 6 |
| CFinnoRay | Sensor akuisisi | 0 dari 18 metode publik, semuanya lewat SDK (semua bagian tanpa alat sudah di `src/sensor.rs`, lihat bagian Sensor) | 18 |
| AnalysisServerAccessor | Layanan cloud | 0 | 6 |
| **Jumlah** | | **57** | **81** |

DicomProcessor (didekompilasi dengan Ghidra) adalah pembungkus DCMTK. IsDicom membaca 4 byte di offset 128 dan membandingkan dengan "DICM". GetUid memanggil `dcmGenerateUniqueIdentifier` dengan root OFFIS `1.2.276.0.7230010.3.1.{2,3,4}` untuk study/series/instance. LumoDent tidak memakai root milik OFFIS dan memakai bentuk `2.25.<angka>` yang tidak butuh registrasi. GetCurrentDateTime memanggil `DcmDateTime::getCurrentDateTime` dengan detik dan pecahan, hasilnya `YYYYMMDDHHMMSS.FFFFFF`. GetLocaleEncoding (pemilihan code page CP1252/UTF-8/GB18030 dan lain-lain) tidak diperlukan karena LumoDent menulis teks UTF-8 langsung. InsertElement belum punya padanan publik; elemen tambahan saat menulis ditentukan di dalam `write`.

DataAccessor sudah lengkap. Dari empat modul layar utama, Patient, Viewer dan Report tersedia, sedangkan Acquisition belum.

## Sensor: pemetaan dari IOSensorAcquisition.dll

CFinnoRay membungkus SDK detektor vendor lewat USB. Bagian yang berbicara ke alat sudah dipetakan, tapi belum ditulis ulang karena butuh SDK atau protokol USB-nya dan sensor fisik untuk dites:

- InitializeByPath (0x100088f0) memuat pustaka SDK dan membuat instance dengan callback SDKCallbackHandler.
- Connect (0x100079b0) membuka koneksi dengan timeout 10000 ms, lalu JudgeSensorSize (0x10008bf0) membaca atribut 0x138f dan 0x1390 (lebar, tinggi).
- GetDectectorState membaca atribut 0x138a. SetTriggerMode menulis atribut 0x9ca dan hanya menerima mode 6 atau 7.
- WaitForImage (0x10009850) menunggu dua event Windows tanpa batas waktu: -1 berarti error detektor, -2 berarti dihentikan.

Seri sensor (tabel di 0x10004f00), sekarang `sensor::MODELS`:

| Kode | Seri | Lebar × tinggi | Chamfer pojok |
|---:|---|---|---:|
| 1000 | NanoPix 1 | 1000 × 1500 | 211 |
| 1500 | NanoPix 1.5 | 1200 × 1650 | 251 |
| 2000 | NanoPix 2 | 1300 × 1800 | 281 |

### NanoPix-E 1.3 (Eighteeth) memakai SDK lain

Dialog registrasi produk (NanoPix.exe 0x583830, `##sensorSize`) punya empat pilihan: NanoPix 1 (0), NanoPix 2 (1), NanoPix 1.5 (2) dan **NanoPix-E 1.3 (3)**. Registrasi di 0x53ab00 membagi jalur: tipe 0 sampai 2 lewat CFinnoRay (IOSensorAcquisition.dll, tabel di atas), tipe 3 lewat jalur 0x50c930 / 0x50e1b0 yang memanggil API `Sensor_*`:

- `sensor_io.dll` (PDB `HDR_MultiDevice`, USB Cypress `CCyUSBEndPoint`): Sensor_OpenDevice, Sensor_GetImageSize, Sensor_GetImageBits, Sensor_GetXrayState, Sensor_SetIntegrationTime, Sensor_SetHdrValue, Sensor_SetBinningMode dan lain-lain.
- `HdrImageCorrect.dll`: Sensor_CorrectImage, Sensor_LoadRawImage, Sensor_GetHdrMode. Ini koreksi gambar murni perangkat lunak, jadi bisa ditulis ulang tanpa alat.

Ukuran sensor NanoPix-E dibaca dari alat (Sensor_GetImageSize), tidak ada di tabel CFinnoRay.

Semua yang tidak butuh alat sudah ada di `src/sensor.rs`:

| Asli | Isi | Rust |
|---|---|---|
| GetAndCaliFile_Gain 0x10007d30 | cari file `*.gn` di `<folder sensor>/Correct/Default`; header 0x410 byte; berisi 4 flat field 16-bit jika ukuran file > w×h×4, selain itu 1 | `find_calibration_file`, `parse_gain_file`, `load_calibration` |
| ReadCaliFile_Gain 0x10008ce0 | level dosis = piksel pertama flat field, lalu CalcGainMat | `Gain::level` |
| CalcGainMat 0x10007740 | gain = rata-rata / piksel, minimal 0.001 | `gain_map` |
| DoGainCorrection 0x10007d00 → 0x100255c0 | 1 kalibrasi: kalikan. 4 kalibrasi: rata-rata 5000 piksel terterang, pilih level terdekat | `bright_level`, `gain_correct` |
| GetBadPixelInfo 0x10008150 | file `*.dft`, header 0x410, w×h byte, bukan nol = rusak | `parse_defect_file` |
| ImgFix 0x10021220 (r=1, mode 6, 200 putaran) | tiap putaran: piksel rusak yang punya tetangga 4-arah sehat diganti rata-rata piksel sehat di jendela 3×3; yang sudah diperbaiki dianggap sehat di putaran berikutnya | `fix_defects` (sama persis) |
| Mask histogram di AcqImageEx | dihitung di bawah dua chamfer atas, lalu dibuang (tidak dipakai aslinya) | `exposure_mask`, untuk diagnosa |
| Kode error AcqImageEx | 0 sampai -10 | `acquisition_error` |
| Pasca-proses AcqImageEx | gain lalu perbaikan piksel | `post_process`, `parse_raw_frame` |

## Antarmuka lintas platform

Antarmuka Win32 ditulis ulang dengan egui 0.37 (eframe, renderer wgpu: Direct3D 12 di Windows termasuk ARM64, Vulkan atau GL di Linux, Metal di macOS). Logika non-UI pindah ke pustaka agar dapat diuji tanpa jendela.

| Lapisan | Berkas | Isi |
|---|---|---|
| Inti | `src/workspace.rs` | Folder data: impor citra, hapus studi, simpan/muat/hapus laporan, info klinik, hapus pasien beserta berkasnya |
| Inti | `src/pdf.rs`, `src/reports.rs` | PDF laporan dan model teks halaman yang dipakai bersama pratinjau |
| Inti | `src/dates.rs` | Waktu lokal lewat `chrono` (sebelumnya `GetLocalTime`) |
| Inti | `src/patients.rs` | SQLite sistem: `winsqlite3` di Windows (raw-dylib), `libsqlite3` di Linux/macOS |
| UI | `src/ui/` | `theme.rs` palet, `icons.rs` ikon vektor dan tombol, `patient.rs`, `viewer.rs`, `report.rs`, `mod.rs` header, navigasi, menu dan dialog |

Dialog konfirmasi dan pesan memakai modal egui dengan teks yang sama seperti versi Win32. Dialog berkas memakai `rfd` (dialog native di Windows dan macOS, xdg-desktop-portal di Linux). Folder data: `%LOCALAPPDATA%\LumoDent` di Windows, `~/Library/Application Support/LumoDent` di macOS, `$XDG_DATA_HOME/lumodent` atau `~/.local/share/lumodent` di Linux; `LUMODENT_DATA` mengganti lokasi.

Uji UI `src/ui/tests.rs` memakai `egui_kittest`: tombol, daftar dan kolom teks ditemukan lewat pohon aksesibilitas, sehingga uji yang sama berlaku di semua OS. Pemeriksaan aksesibilitas bawaan kittest memastikan setiap kontrol punya nama. `render_screens` (diabaikan secara default) merender setiap layar ke `target/screens` dengan data demo.

## DICOM

`src/dicom.rs` membaca dan menulis berkas DICOM Part 10 tanpa pustaka DICOM pihak ketiga.

- Baca: Implicit VR LE, Explicit VR LE, Deflated LE, JPEG baseline/extended (8-bit), dan dataset tanpa header Part 10. Piksel 8/16-bit, signed, bits stored, rescale slope/intercept, window center/width, MONOCHROME1/2, RGB (interleaved dan planar). Sequence dilewati. Belum: big endian, JPEG lossless, JPEG 2000, RLE, multi-frame JPEG. Berkas multi-frame native hanya menampilkan frame pertama.
- Tulis: Explicit VR LE 8-bit. Gambar abu-abu menjadi Digital Intra-Oral X-Ray (For Presentation, modality IO), gambar berwarna menjadi Secondary Capture. Atribut pasien dan studi diambil dari database; UID memakai root `2.25`.
- Import `.dcm` mengisi SOP Instance UID, modality, accession number, body part, deskripsi dan waktu studi dari berkas. Bila Patient ID di berkas berbeda, gambar tetap diimpor dan muncul peringatan.
- Belum diuji dengan berkas DICOM dari sensor atau PACS sungguhan; tes memakai berkas buatan sendiri.

## ImageProcessing: pemetaan dari CoreLibrary.dll

Sumber: disassembly `CoreLibrary.dll` 1.1.1.9 (export thunk → fungsi asli) dan call site di `NanoPix.exe` dengan `llvm-objdump`, lalu decompile Ghidra 12.2 lewat REA untuk ImageNormalizeEx dan MatTransfor. Referensi bekerja pada piksel abu 16 bit (format 4); LumoDent memakai 8 bit sehingga konstanta rentang diskalakan ke 0..255. Hasil di-clamp lalu dipotong (truncate), seperti referensi.

| Fungsi | Alamat | Algoritma asli | Parameter yang dipakai NanoPix.exe |
|---|---|---|---|
| Smoothness | 0x100c3410 → 0x100c1be0 | Kernel Gaussian (2r+1)² `exp(-(dx²+dy²)/2σ²)` dinormalisasi; konvolusi dengan padding reflect-101 (separable bila rank 1) | Viewer 0x521f76: r=2, σ=4 |
| Sharpness | 0x100c3300 → 0x100c1760 | Kernel Gaussian tak dinormalisasi (jumlah S); `v·(1+aS) − a·(K∗v)`; mask opsional dari `|v−blur| ≥ threshold` | Viewer 0x521f31: r=1, a=0.6·level (+2 untuk nilai sensor 1200/1600), σ=1, tanpa mask; lalu MedianSmooth(1) |
| MedianSmooth | 0x100c21f0 → 0x100bb560 | Median jendela (2r+1)², padding reflect-101 | Viewer r=1; akuisisi r=1/2 |
| MultScaleWLN | 0x100c2460 → 0x100b9af0 | LUT rata-rata n jendela linear: jendela k memetakan `[min+k(A−min)/n, max+k(B−max)/n]` ke `[C,D]` | Akuisisi 0x4f3f41: n=10, A=10000 (atau 14660), B=65535, C=0, D=65535 |
| BrightnessAndContrast | 0x100bf0a0 | Jendela pusat `32767.5 − 163.84·b`, lebar `65536/e^(c/100)`, ditarik linear ke 0..65535 | Slider ±100 |
| AutoBrightnessAndContrast | 0x100be720 | Histogram di jendela B/C, pangkas sisi yang bagian luarnya lebih sedikit sampai (1−ratio) piksel terbuang, lalu kembali ke slider: `c = ln(65536/(hi−lo))·100 + 0.5`; slider di batas → reset 0 | 0x575152: ratio 0.97, batas 100/100 |
| Invert | 0x100c1ed0 | `v = min(param, TYPE_MAX) − v` | Viewer memberi nilai max citra |
| GetMinMaxIntegerValue | 0x100c0b70 | Min/maks piksel | — |
| ImageNormalizeEx | 0x100c1270 → 0x100bc310 (8 bit), 0x100bc670 (16 bit) | `clamp((v−min)·(outMax−outMin)/(max−min), outMin, outMax)`, dipotong; outMin tidak ditambahkan sebagai offset; default 0..255 / 0..65535 | 7 call site (akuisisi, laporan) |
| MatTransfor | 0x100c2080 → 0x100bf590 | Permutasi piksel dengan matriks integer: (x,y) → (a·x+b·y+c, d·x+e·y+f), ukuran output abs(a·w+b·h) × abs(d·w+e·h); hanya format 16 bit | Matriks dari GetFlipTransMatrix |

Belum dipetakan (rumus rewrite sendiri): Set, MultiplyConstant, GetAverageValue. GetMinMaxFloatValue hanya untuk citra float (format 6) dan tidak dipakai jalur 16 bit. Fungsi tambahan yang ada di DLL tapi tidak diimpor NanoPix.exe: AddConstant, Copy, DrawImageHists, GammaCorrect, WindowWidthLevel.

## DataAccessor: basis data User dan Analysis

Didecompile dengan Ghidra 12.2 lewat REA (CreateMainDB 0x10037050, CreateUserDB 0x10037530, CheckUserDB 0x10036df0, AddUser 0x100369e0, GetUsers 0x1003b8a0); statement SQL diambil dari literal di CoreLibrary.dll.

| Basis data | Tabel | user_version | File LumoDent |
|---|---|---|---|
| Utama (CreateMainDB) | Patient, Study, Report, Analysis | 1 | `patients.sqlite3` |
| User (CreateUserDB) | Product, User, CloudUser | 0 | `users.sqlite3` |

- Analysis: satu nomor perawatan (treatmentNo) per studi; Update/Delete dikunci oleh `patient_id` + `study_pk`, Get per `study_pk`.
- CloudUser: satu akun per produk; Update/Delete/Get dikunci oleh `product_pk`, terhapus ikut produknya (ON DELETE CASCADE).
- CheckUserDB referensi membuat ulang basis data bila tabel Product/User hilang dan meng-upgrade versi < 4 tanpa CloudUser. LumoDent cukup membuat tabel yang belum ada (`CREATE TABLE IF NOT EXISTS`), sehingga data lama tidak terhapus.
- NanoPix.exe (0x56f2b5) membuat admin pertama "Admin" / "123456" dan menyimpan `user_password` sebagai teks biasa; `encryption.dll` hanya dipakai untuk kunci produk dan kode hardware. LumoDent membuat admin yang sama, tetapi menyimpan SHA-256 bergaram 20.000 putaran (`src/sha256.rs`, tanpa crate). Password teks biasa dari basis data NanoPix tetap diterima dan langsung di-hash pada login pertama yang berhasil.
- Belum ada layar login, manajemen user, maupun layar produk di antarmuka; fungsi ini baru tersedia di lapisan data.

## NanoPix-E 1.3: HdrImageCorrect.dll dan Scilib21.dll

Semua koreksi gambar NanoPix-E sekarang ada di `src/hdr.rs`, tanpa DLL. `Sensor_CorrectImage` (0x10001060) menerima frame 16-bit big-endian, nomor seri sensor (maksimal 8 karakter), lebar, tinggi, flag HDR dan folder kalibrasi. Scilib21.dll adalah pustaka ShadoCam (Rad-icon), terlihat dari header file pixel map.

| Asli | Isi | Rust |
|---|---|---|
| Nama file | `<folder>\<seri>_Offset Image`, `_Gain Image`, `_Pixel Map`; mode HDR `_Offset Image0/1`, `_Gain Image0/1` | `calibration_path` |
| File offset dan gain | w×h nilai 16-bit big-endian; ukuran salah membuat koreksi itu dilewati | `parse_image_file` |
| ScOffsetCorrection 0x100024d0 | piksel − offset, 16-bit wrapping | `offset_correction` |
| ScGainCorrection 0x10002510 (dari assembly) | rata-rata gain (signed 16-bit); piksel × rata-rata / gain, dipotong ke arah nol; gain 0 → 0 | `gain_correction` |
| ScReadPixMap 0x100036a0 | teks diawali "ShadoCam Pixel Map File", entri setelah "$begin": `C x,y0,y1,flag` (kolom), `P x,y`, `R y,x0,x1,flag` (baris), koordinat mulai dari 1; maksimal 0x19000 byte dan 10000 entri; koordinat 0 = error sintaks (kode 8) | `parse_pixel_map` (meniru perilaku strtoul aslinya) |
| ScCalcPixMapMask 0x10002c30 | bit tetangga yang rusak; untuk entri P hanya entri sesudahnya yang dilihat (perilaku asli) | `calc_masks` |
| ScFixPixel 0x10002620 | mode 0 rata-rata 8 tetangga sehat; 1 horizontal; 2 vertikal (bobot 2 untuk tetangga langsung, 1 untuk yang berjarak 2); 3 dan 4 memilih arah dengan selisih kuadrat terkecil dari tiga arah, lalu interpolasi berbobot (3−a)/(3−b) | `fix_pixel` |
| ScPixelCorrection 0x10003220 (metode 2) | kolom dan baris: tiga piksel di tiap ujung memakai arah sederhana, sisanya mode 3 (kolom) atau 4 (baris); piksel tunggal mode 0 | `pixel_correction` |
| HDR 0x10002000 | lihat di bawah | `hdr_merge`, `detect_bit_depth` |
| Sensor_CorrectImage | offset, gain (atau HDR), lalu pixel map; error -1, -2, -3 | `correct_image` |

Mode HDR: bit terendah piksel mentah menandai kanalnya (ganjil = kanal 1, 9.19× kurang sensitif). Kedalaman bit ditebak dari rata-rata 10 baris pertama offset kanal 0 (≥1500 → 16 bit, ≥400 → 14, selain itu 12). Piksel ganjil yang terisolasi (tidak ada piksel ganjil lain di 3×3) dipasangkan dengan rata-rata 8 tetangganya untuk menghitung offset kanal 1, ditambah 200/50/12 sesuai kedalaman bit (2000 bila pasangan kurang dari dua). Lalu setiap piksel: ×0.5, koreksi offset per kanal (kanal 1 ×9.19), dibatasi minimal 0, dibagi gain ternormalisasi (nilai di bawah 0.1 dianggap 1), kanal 1 ditambah offsetnya, ×0.5, dibulatkan menjauhi nol, dibatasi nilai maksimum kedalaman bit.

Satu perbedaan yang disengaja: aslinya memberi ScPixelCorrection jumlah entri = (w×h) & 0x7fff, bukan jumlah entri sebenarnya, sehingga sisa isi buffer gain bisa terbaca sebagai entri palsu. LumoDent hanya memakai entri yang ada di file.

Belum diketahui: flag HDR mana yang dipakai NanoPix.exe untuk NanoPix-E, dan apakah frame dari sensor_io.dll memang big-endian (aslinya menukar byte sebelum dan sesudah koreksi).

Diuji dengan data asli: folder `calibration13` di instalasi NanoPix berisi kalibrasi HDR sensor 12090806 (Offset/Gain Image0/1 berukuran 1200 × 1600, Pixel Map 7.228 entri). Tes `real_calibration_folder` (aktif bila `LUMODENT_CAL_DIR` dan `LUMODENT_CAL_SN` diisi) membaca semuanya: kedalaman terdeteksi 14 bit, dan flat field setelah HDR merge dan perbaikan piksel rata dengan simpangan 3,3%. Data kalibrasi tidak di-commit.

## Pengaturan (NanoPix.exe 0x583830)

Dialog Settings asli punya lima tab; semua field dan daftar pilihannya dipetakan ke `src/settings.rs` (disimpan di `settings.txt`) dan `src/ui/settings.rs` (menu Alat → Pengaturan…). Daftar pilihan diambil dari tabel string di bagian data executable.

| Tab | Fungsi asli | Isi | Dipakai di LumoDent |
|---|---|---|---|
| Umum | 0x58aed0 | bahasa, ukuran ikon (kecil/sedang/besar), nama/alamat/telepon klinik (128/320/16 karakter) | bahasa dan tema, zoom antarmuka 0,9/1,0/1,15, data klinik untuk laporan |
| Pasien | 0x58b8f0 | nomor rekam medis otomatis, urutan nama, hari untuk Baru Diambil/Baru Dicetak | nomor otomatis mengisi ID pasien baru dengan stempel waktu; lainnya disimpan |
| Akuisisi | 0x589a50 | generator DC/AC, kV 60–80, mA (DC: ≤0,2 … >3,0; AC: <4,0 / 4,0~7,0 / >7,0), kalibrasi mm/piksel 0,005–1,0, lokasi data | kalibrasi dipakai penggaris bila gambar tidak menyimpan pixel spacing; generator disimpan untuk akuisisi |
| Penampil | 0x58d2d0 | layar penuh, layout 1x1/1x2/2x1/2x2, kaca pembesar 100–200 px ×1,5–2,5, warna bingkai tangkapan, grid 1/2/5/10 mm (warna, tebal 1–5 px, opasitas 10–50%), warna dan tebal panjang/sudut/gambar/garis/persegi/elips, tampil satuan, gaya memo dan 7 teks cepat | layar penuh saat start, warna/tebal/satuan penggaris; lainnya disimpan untuk fitur yang belum ada |
| Laporan | 0x58c3c0 | warna dan tebal bingkai, ukuran huruf 9–18 pt, warna teks (aslinya juga ukuran kertas, orientasi, header, footer) | disimpan; ukuran kertas, orientasi, header dan footer tetap diatur per laporan |

Saat jenis generator diganti, aslinya mengatur ulang kV dan mA (DC → mA indeks 3, AC → indeks 1); `Settings::set_generator_ac` meniru itu. Algoritma nomor rekam medis otomatis aslinya belum dipetakan.

## Alat Penampil dan layar Akuisisi

Fitur yang tadinya hanya tersimpan di Pengaturan sekarang dipakai:

- **Alat di bilah bawah Penampil** (`Tool`): Geser, Ukur, Garis, Persegi, Elips, Gambar Bebas, Memo, Kaca Pembesar. Klik alat yang aktif untuk kembali ke Geser; Esc juga. Klik kanan menghapus anotasi terakhir (di mode Ukur: penggaris terakhir). Tombol Hapus Anotasi membersihkan semuanya. Warna dan tebal tiap bentuk diambil dari Pengaturan.
- **Memo**: klik di gambar membuka kotak teks dengan tombol Teks Cepat dari 7 preset di Pengaturan; warna kotak dan teks dari Gaya Memo.
- **Grid** (tombol atau tombol G): garis tiap 1/2/5/10 mm sesuai skala mm/piksel gambar (atau kalibrasi), dengan warna, tebal dan opasitas dari Pengaturan; tidak digambar bila jaraknya di layar kurang dari 4 px.
- **Kaca pembesar**: lensa persegi 100/150/200 px di bawah kursor, perbesaran 1,5/2,0/2,5× terhadap tampilan saat ini, bingkai memakai warna Bingkai Tangkapan.
- **Layout 1x1, 1x2, 2x1, 2x2** (tombol layout atau Pengaturan): sel pertama berisi gambar yang dibuka dengan semua alat, sel lain menampilkan gambar berikutnya (diperkecil maksimal 1024 px); klik sel untuk membukanya.
- **Layar Akuisisi**: panel generator DC/AC, kV dan mA seperti NanoPix, langsung tersimpan ke Pengaturan. Pengambilan gambar tetap menunggu driver sensor.

Anotasi belum ikut tercetak saat Kirim ke Laporan atau Ekspor; itu disimpan terpisah dari piksel gambar.
