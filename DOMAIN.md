# Pemetaan data lokal NanoPix 1.1.1.9

## Bukti statis

Target utama adalah PE x86 `NanoPix.exe` dengan SHA-256 `1125f89f6c7b31a4c7674dc225ed89dfcba38a6a97072306c16d1346954af739`. Tabel nama impor memuat metode kelas `DataAccessor`. Implementasi berada di `CoreLibrary.dll` dengan SHA-256 `4dfaf822448f23e031aef530e2ef27272f7fe824b3e1eb00434abaa22510c1ff` dan image base `0x10000000`.

Alamat di bawah berupa RVA heksadesimal. Direktori ekspor PE menunjuk ke instruksi JMP rel32. Kolom ketiga menunjukkan tujuan JMP. Di dalam setiap rutin, instruksi PUSH imm32 pada RVA berikutnya merujuk literal SQL dengan offset berkas di kolom kelima. Pemetaan ini menghubungkan simbol, kode, dan kueri secara statis. Jalur kegagalan dan seluruh cabang asli belum diuji.

| Metode DataAccessor | Ekspor | Implementasi | PUSH SQL | Offset SQL | Perilaku yang didukung bukti | Implementasi Rust |
|---|---:|---:|---:|---:|---|---|
| AddPatient | 36b6 | 36190 | 3620b | 143fe8 | Menambah pasien dengan waktu dibuat dan diubah | Ada |
| DeletePatient | 269e | 37bb0 | 37c38 | 144154 | Menghapus pasien berdasarkan PK | Ada untuk tabel pasien |
| UpdatePatient | 53d0 | 3c110 | 3c18b | 144180 | Mengubah sepuluh atribut serta waktu pembaruan, mempertahankan ID | Ada |
| GetPatients | 2fcc | 389e0 | 38abc | 1442b0 | Mencari ID, nama belakang, nama depan, atau telepon dan menyaring rentang waktu pembuatan inklusif | Pencarian dasar dan tanggal sudah ada. Cabang modalitas belum ada |
| GetRecentlyAquiredPatients | 68fc | 3a0d0 | 3a153 | 1445f0 | Mengambil pasien dengan Study yang dibuat pada rentang waktu | Ada |
| GetRecentlyPrintedPatients | 28e7 | 3a750 | 3a7d3 | 144690 | Mengambil pasien dengan Report yang dibuat pada rentang waktu | Ada |
| AddStudy | 21f8 | 36750 | 367cb | 144730 | Menyimpan metadata studi dengan ID pasien, SOP Instance UID, nama dan jenis citra | Ada |
| GetStudies | 1618 | 3b170 | 3b24d | 1449d0 | Menyaring studi menurut modalitas, pasien, dan waktu dibuat | Ada |
| AddReport | 25a9 | 365e0 | 3665b | 144b18 | Membuat rekam laporan dari ID pasien dan nama proyek | Ada, tanpa UI |
| GetReports | 691a | 3add0 | 3ae53 | 144c40 | Menyaring laporan menurut pasien dan waktu dibuat | Ada, tanpa UI |

Fragmen DDL Patient mulai di offset berkas `0x143808`. Skema mendefinisikan PK integer, ID unik, nama belakang, nama depan, jenis kelamin, tanggal lahir, dokter, ID sosial, nomor telepon, email, kode pos, alamat dan dua timestamp. Nama tabel, batasan PK/ID, dan kolom diimplementasikan dalam Rust. Literal LIKE dan tanda persen tampak di biner. Rewrite memilih pencocokan substring dengan pola SQLite, serta urutan PK untuk hasil yang stabil. Urutan tampilan asli masih perlu diobservasi.

## Verifikasi implementasi

`src/patients.rs` menulis data melalui SQLite sistem (winsqlite3 di Windows, libsqlite3 di Linux/macOS) dari Rust menggunakan parameter kueri terikat. Fungsi tambah, ubah, hapus, pencarian empat atribut dan filter waktu tersedia dari `PatientStore`. Aplikasi dapat membuka berkas terpisah untuk menyimpan data antarsesi. Waktu lokal diambil dari Rust (`chrono`) karena pengujian awal dengan SQLite sistem mengembalikan NULL untuk ekspresi waktu lokal pada host ini.

Pengujian `patients::tests::add_search_edit_delete_and_reopen` menggunakan database sementara dengan data sintetis. Uji mencakup penolakan ID duplikat, nama dengan apostrof, pencarian setiap atribut, penyaringan waktu, pengubahan telepon, pembukaan ulang berkas, penghapusan, dan PK yang sudah hilang. Kombinasi Rust GNU LLVM dan SQLite sistem Windows lulus uji ini. Kesetaraan data lama perlu diuji pada salinan basis data contoh dengan skema dan versi yang diketahui. Mekanisme penyimpanan berkas radiografi, pemrosesan piksel dan akuisisi sensor tetap terbuka.

Uji kedua, `patients::tests::recent_activity_uses_study_and_report_dates_independently`, memverifikasi filter berdasarkan waktu pembuatan Study atau Report. Basis data baru mengembalikan daftar kosong. Fixture berisi dua aktivitas pasien yang sama, aktivitas tepat di kedua batas rentang, serta aktivitas di luar rentang. Hasil menampilkan pasien sekali per filter dan memilih berdasarkan waktu aktivitas, terlepas dari waktu pembuatan pasien. Bukti asli hanya menetapkan keberadaan rekam Report dalam rentang sebagai kriteria "Recently Printed". Keberhasilan cetak fisik belum diobservasi.

## Preset tanggal

Kesembilan pilihan menghasilkan batas lokal inklusif untuk ketiga pencarian. Implementasi awal memilih kemarin sebagai satu hari penuh, Three Days sebagai hari ini dan dua hari sebelumnya, This Week sebagai Senin sampai hari ini, Last Week sebagai Senin sampai Minggu sebelumnya, This Month sebagai tanggal satu sampai hari ini, dan Last Month sebagai seluruh bulan sebelumnya. Three Months dan Six Months mulai pada tanggal yang sama tiga atau enam bulan sebelumnya sampai hari ini, dengan penyesuaian ke akhir bulan yang lebih pendek. All Dates menerima seluruh rentang timestamp yang valid. Nama preset dan filter SQL didukung artefak. Pilihan awal minggu, jangka waktu bergulir, dan batas atas hari ini masih menunggu pembandingan perilaku dengan biner asli pada data uji.

Dua uji kalender mencakup semua preset, pergantian tahun, minggu yang melintasi bulan, akhir Februari tahun kabisat serta tahun 2100, dan indeks preset di luar daftar. Seluruh empat pengujian pustaka lulus pada Windows.

## Study dan Report

Literal DDL Study dan Report berada setelah DDL Patient. UpdateStudy, DeleteStudy, UpdateReport dan DeleteReport juga ada sebagai literal SQL dan diimplementasikan bersama empat metode di tabel atas. Tabel Analysis, Product, User dan CloudUser sudah terlihat di DLL tetapi belum dibuat oleh rewrite.

Uji `studies::tests::studies_filter_by_patient_and_modality_and_cascade_on_patient_delete` memeriksa penolakan FK untuk pasien yang tidak ada, penyaringan pasien dan modalitas, rentang waktu, pembaruan, penghapusan berulang, Report, serta penghapusan berantai saat pasien dihapus. Uji `folder_names_are_single_safe_components` memastikan nomor rekam medis dengan karakter terlarang Windows atau nama perangkat cadangan tetap menjadi satu komponen folder.
