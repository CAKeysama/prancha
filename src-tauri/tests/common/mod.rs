//! Geradores de arquivos de teste próprios (nenhum quadrinho real é usado).
#![allow(dead_code)]

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

/// Imagem PNG com cor sólida + faixa, para distinguir páginas.
pub fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = RgbaImage::from_fn(w, h, |x, y| {
        if y < h / 10 { Rgba([seed, 255 - seed, 40, 255]) } else { Rgba([(x % 200) as u8, seed, (y % 200) as u8, 255]) }
    });
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(img).write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

pub fn jpeg(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([seed, (x % 255) as u8, (y % 255) as u8]));
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img).write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

pub type Files<'a> = &'a [(&'a str, Vec<u8>)];

pub fn make_cbz(path: &Path, files: Files) -> PathBuf {
    let f = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    path.to_path_buf()
}

pub fn make_cbt(path: &Path, files: Files) -> PathBuf {
    let f = std::fs::File::create(path).unwrap();
    let mut tar = tar::Builder::new(f);
    for (name, data) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, data.as_slice()).unwrap();
    }
    tar.finish().unwrap();
    path.to_path_buf()
}

pub fn make_cb7(path: &Path, files: Files) -> PathBuf {
    let mut w = sevenz_rust2::ArchiveWriter::create(path).unwrap();
    for (name, data) in files {
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(Cursor::new(data.clone()))).unwrap();
    }
    w.finish().unwrap();
    path.to_path_buf()
}

pub fn make_folder(path: &Path, files: Files) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    for (name, data) in files {
        std::fs::write(path.join(name), data).unwrap();
    }
    path.to_path_buf()
}

// ---------------------------------------------------------------------------
// RAR5 "stored" (sem compressão), segundo a especificação pública do formato
// (https://www.rarlab.com/technote.htm). Não existe compressor RAR livre;
// este gerador permite testar a leitura real pela biblioteca UnRAR.
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn vint(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

/// `body` começa no campo "header type". Prefixa CRC32 e tamanho do cabeçalho.
fn rar_block(body: &[u8]) -> Vec<u8> {
    let mut size = Vec::new();
    vint(body.len() as u64, &mut size);
    let mut crc_input = size.clone();
    crc_input.extend_from_slice(body);
    let mut out = crc32(&crc_input).to_le_bytes().to_vec();
    out.extend_from_slice(&crc_input);
    out
}

pub fn make_rar5(path: &Path, files: Files) -> PathBuf {
    let mut out: Vec<u8> = b"Rar!\x1a\x07\x01\x00".to_vec();
    // Cabeçalho principal: tipo 1, flags 0, archive flags 0.
    let mut main = Vec::new();
    vint(1, &mut main);
    vint(0, &mut main);
    vint(0, &mut main);
    out.extend(rar_block(&main));
    for (name, data) in files {
        let mut h = Vec::new();
        vint(2, &mut h); // tipo: arquivo
        vint(0x0002, &mut h); // flags: área de dados presente
        vint(data.len() as u64, &mut h); // tamanho dos dados
        vint(0x0004, &mut h); // file flags: CRC32 presente
        vint(data.len() as u64, &mut h); // tamanho descompactado
        vint(0x20, &mut h); // atributos
        h.extend_from_slice(&crc32(data).to_le_bytes());
        vint(0, &mut h); // compressão: versão 0, método 0 (store)
        vint(0, &mut h); // SO: Windows
        vint(name.len() as u64, &mut h);
        h.extend_from_slice(name.as_bytes());
        out.extend(rar_block(&h));
        out.extend_from_slice(data);
    }
    let mut end = Vec::new();
    vint(5, &mut end);
    vint(0, &mut end);
    vint(0, &mut end);
    out.extend(rar_block(&end));
    std::fs::write(path, out).unwrap();
    path.to_path_buf()
}

/// PDF mínimo válido com `pages` páginas em branco e dicionário Info.
pub fn make_pdf(path: &Path, pages: usize, title: Option<&str>) -> PathBuf {
    let mut objects: Vec<String> = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages));
    for _ in 0..pages {
        objects.push("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>".into());
    }
    let info_id = objects.len() + 1;
    objects.push(match title {
        Some(t) => format!("<< /Title ({t}) /Author (Autora Teste) >>"),
        None => "<< /Producer (Prancha testes) >>".into(),
    });
    let mut body = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(body.len());
        body.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, obj));
    }
    let xref = body.len();
    body.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in offsets {
        body.push_str(&format!("{off:010} 00000 n \n"));
    }
    body.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        info_id,
        xref
    ));
    std::fs::write(path, body).unwrap();
    path.to_path_buf()
}

/// Páginas `1.png`, `2.png`... (nomes com vida estática para simplificar os testes).
pub fn pages(n: usize) -> Vec<(&'static str, Vec<u8>)> {
    (0..n)
        .map(|i| {
            let name: &'static str = Box::leak(format!("{}.png", i + 1).into_boxed_str());
            (name, png(60, 90, (i * 20) as u8))
        })
        .collect()
}

pub fn as_files(v: &[(&'static str, Vec<u8>)]) -> Vec<(&'static str, Vec<u8>)> {
    v.to_vec()
}
