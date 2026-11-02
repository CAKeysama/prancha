//! Esquema `prancha://` (no Windows: `http://prancha.localhost/`).
//! Só serve conteúdo de itens do catálogo, identificados por id — nunca caminhos arbitrários.
//!
//! - `/cover/<id>`        miniatura da capa
//! - `/page/<id>/<i>`     página `i` (0-based) de HQs de imagens
//! - `/file/<id>`         o PDF original, com suporte a `Range` para o PDF.js
//! - `/online-cover/<id>` miniatura de uma obra de uma fonte, baixada aqui e guardada em cache

use std::io::{Read, Seek, SeekFrom};

use tauri::http::{Request, Response, StatusCode, header};

use crate::core::Core;
use crate::error::{AppError, AppResult};
use crate::extensions::sources;
use crate::library;

type Resp = Response<Vec<u8>>;

fn base(status: StatusCode) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range, Accept-Ranges, Content-Length, X-Prancha-Error")
}

fn error(status: StatusCode, code: &str, message: &str) -> Resp {
    base(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("X-Prancha-Error", code)
        .body(message.as_bytes().to_vec())
        .unwrap()
}

fn status_for(code: &str) -> StatusCode {
    match code {
        "not_found" | "page_not_found" => StatusCode::NOT_FOUND,
        "file_missing" => StatusCode::GONE,
        "permission_denied" => StatusCode::FORBIDDEN,
        "too_large" => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

pub fn handle(core: &Core, req: &Request<Vec<u8>>) -> Resp {
    if req.method() == tauri::http::Method::OPTIONS {
        return base(StatusCode::NO_CONTENT)
            .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, HEAD, OPTIONS")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
            .body(Vec::new())
            .unwrap();
    }
    let segments: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
    let id = segments.get(1).and_then(|v| v.parse::<i64>().ok());
    let index = segments.get(2).and_then(|v| v.parse::<usize>().ok());
    let served = match (segments[0], id, index) {
        ("cover", Some(id), _) => cover(core, id),
        ("page", Some(id), Some(index)) => page(core, id, index),
        ("file", Some(id), _) => pdf_file(core, id, req),
        ("online-cover", Some(id), _) => online_cover(core, id),
        ("cover" | "page" | "file" | "online-cover", ..) => return error(StatusCode::BAD_REQUEST, "bad_request", "Requisição inválida."),
        _ => return error(StatusCode::NOT_FOUND, "not_found", "Recurso não encontrado."),
    };
    served.unwrap_or_else(|e| error(status_for(e.code()), e.code(), &e.to_string()))
}

/// O arquivo sumiu do disco: a HQ passa a aparecer como ausente.
fn note_missing(core: &Core, id: i64, e: impl Into<AppError>) -> AppError {
    let e = e.into();
    if e.code() == "file_missing" {
        let _ = library::set_missing(&core.db(), id, true);
    }
    e
}

fn cover(core: &Core, id: i64) -> AppResult<Resp> {
    let info = library::file_info(&core.db(), id)?;
    let Some(file) = info.cover_file.filter(|f| !f.is_empty() && !f.contains(['/', '\\']) && !f.contains("..")) else {
        return Ok(error(StatusCode::NOT_FOUND, "no_cover", "Esta HQ não possui miniatura em cache."));
    };
    Ok(match std::fs::read(core.paths.covers.join(file)) {
        Ok(bytes) => base(StatusCode::OK)
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(bytes)
            .unwrap(),
        Err(_) => error(StatusCode::NOT_FOUND, "no_cover", "A miniatura não está mais no cache."),
    })
}

fn online_cover(core: &Core, id: i64) -> AppResult<Resp> {
    Ok(match sources::cover(core, id)? {
        // A fonte pode trocar a capa de uma obra: a validade é curta, ao contrário das capas da biblioteca.
        Some(bytes) => base(StatusCode::OK).header(header::CONTENT_TYPE, "image/jpeg").header(header::CACHE_CONTROL, "public, max-age=3600").body(bytes).unwrap(),
        None => error(StatusCode::NOT_FOUND, "no_cover", "Esta obra não possui capa disponível."),
    })
}

fn page(core: &Core, id: i64, index: usize) -> AppResult<Resp> {
    let info = library::file_info(&core.db(), id)?;
    let (bytes, mime) = core.pages.read_page(&info, index).map_err(|e| note_missing(core, id, e))?;
    Ok(base(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "private, max-age=3600")
        .body(bytes)
        .unwrap())
}

/// Interpreta `Range: bytes=a-b` (um único intervalo).
fn parse_range(value: &str, len: u64) -> Option<(u64, u64)> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len.checked_sub(1)?)
        }
        (s, "") => (s.parse().ok()?, len.checked_sub(1)?),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len.checked_sub(1)?)),
    };
    if start > end || start >= len { None } else { Some((start, end)) }
}

/// Limite por resposta para não carregar PDFs enormes inteiros na memória.
const MAX_RANGE_CHUNK: u64 = 32 * 1024 * 1024;

fn pdf_file(core: &Core, id: i64, req: &Request<Vec<u8>>) -> AppResult<Resp> {
    let info = library::file_info(&core.db(), id)?;
    if info.format != "pdf" {
        return Ok(error(StatusCode::NOT_FOUND, "not_found", "Recurso não encontrado."));
    }
    let mut file = std::fs::File::open(&info.path).map_err(|e| note_missing(core, id, e))?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let range = req.headers().get(header::RANGE).and_then(|v| v.to_str().ok()).map(|v| parse_range(v, len));
    let (status, start, end) = match range {
        Some(Some((s, e))) => (StatusCode::PARTIAL_CONTENT, s, e.min(s + MAX_RANGE_CHUNK - 1)),
        Some(None) => {
            return Ok(base(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .unwrap());
        }
        // shortcut: sem Range o arquivo inteiro vai para a memória, e é assim o primeiro pedido do PDF.js; para PDFs de vários GB, abrir com PDFDataRangeTransport.
        None => (StatusCode::OK, 0, len.saturating_sub(1)),
    };
    let mut buf = vec![0u8; if len == 0 { 0 } else { (end - start + 1) as usize }];
    if file.seek(SeekFrom::Start(start)).and_then(|_| file.read_exact(&mut buf)).is_err() {
        return Ok(error(StatusCode::UNPROCESSABLE_ENTITY, "io", "Falha ao ler o PDF."));
    }
    let mut b = base(status)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, buf.len().to_string())
        .header(header::CACHE_CONTROL, "no-store");
    if status == StatusCode::PARTIAL_CONTENT {
        b = b.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    Ok(b.body(buf).unwrap())
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=0-5000", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None);
        assert_eq!(parse_range("items=0-1", 1000), None);
    }
}
