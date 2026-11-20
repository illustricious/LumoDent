//! Gives the Windows executable its file icon: renders the tooth mark from
//! src/logo.rs into a .ico (PNG-compressed entries) and links it as a
//! resource with llvm-windres / windres. Without a resource compiler the
//! build goes on and only the window icon (set at run time) is shown.

#[allow(dead_code)]
#[path = "src/logo.rs"]
mod logo;

use std::path::PathBuf;
use std::process::Command;

/// A minimal PNG writer (stored zlib blocks), enough for icon entries.
fn png(n: usize, rgba: &[u8]) -> Vec<u8> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        b << 16 | a
    }
    let mut raw = Vec::new();
    for row in rgba.chunks_exact(n * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend(adler(&raw).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend(crc(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend((n as u32).to_be_bytes());
    ihdr.extend((n as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

fn ico() -> Vec<u8> {
    let sizes = [16usize, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&n| png(n, &logo::render(n, true, logo::TOP, logo::BOTTOM))).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (n, img) in sizes.iter().zip(&images) {
        let b = if *n >= 256 { 0 } else { *n as u8 };
        out.extend([b, b, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in images {
        out.extend(img);
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=src/logo.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico_path = out.join("lumodent.ico");
    std::fs::write(&ico_path, ico()).unwrap();
    let rc = out.join("lumodent.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico_path.display().to_string().replace('\\', "/"))).unwrap();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if env == "msvc" {
        let res = out.join("lumodent.res");
        if Command::new("llvm-rc").arg("/fo").arg(&res).arg(&rc).status().map(|s| s.success()).unwrap_or(false) {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
        return;
    }
    let obj = out.join("lumodent-res.o");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let triple = match arch.as_str() {
        "aarch64" => "aarch64-w64-mingw32",
        "x86" => "i686-w64-mingw32",
        _ => "x86_64-w64-mingw32",
    };
    for tool in ["llvm-windres", "windres", "x86_64-w64-mingw32-windres"] {
        let mut cmd = Command::new(tool);
        if tool == "llvm-windres" {
            cmd.arg(format!("--target={triple}"));
        }
        let ok = cmd.arg(&rc).arg("-O").arg("coff").arg("-o").arg(&obj).status().map(|s| s.success()).unwrap_or(false);
        if ok {
            println!("cargo:rustc-link-arg-bins={}", obj.display());
            return;
        }
    }
    println!("cargo:warning=No resource compiler found; lumodent.exe gets no file icon.");
}
