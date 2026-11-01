//! Persistência da Leitura Guiada. O resultado automático e as correções ficam separados: refazer
//! a análise nunca toca no que o usuário corrigiu, e as correções têm prioridade na leitura.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::order::reading_sequence;
use super::{DETECTOR_VERSION, LOW_CONFIDENCE, PageAnalysis, Rect, RegionKind};
use crate::db::now_ms;
use crate::error::{AppError, AppResult};

/// Menor lado aceito numa região corrigida, em fração da página.
const MIN_SIDE: f32 = 0.005;
const MAX_REGIONS: usize = 200;
const MAX_OUTLINE: usize = 200;

/// Região como a interface recebe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidedRegion {
    pub id: String,
    pub page_index: i64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub kind: String,
    pub order: i64,
    /// `None` em regiões desenhadas à mão.
    pub confidence: Option<f32>,
    /// auto, adjusted, added ou confirmed.
    pub review: String,
    /// Contorno do balão em frações da região; vazio em regiões sem forma conhecida.
    #[serde(default)]
    pub outline: Vec<[f32; 2]>,
}

impl GuidedRegion {
    pub fn rect(&self) -> Rect {
        Rect { x: self.x, y: self.y, w: self.w, h: self.h }
    }
}

/// Estado de uma página na Leitura Guiada.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidedPage {
    pub page_index: i64,
    /// auto ou user.
    pub source: String,
    pub regions: Vec<GuidedRegion>,
    pub panels: Vec<Rect>,
    /// Há região abaixo do limite de confiança numa sequência ainda não confirmada.
    pub needs_review: bool,
}

fn select_regions(conn: &Connection, comic: i64, page: i64, source: &str) -> AppResult<Vec<GuidedRegion>> {
    let mut stmt = conn.prepare_cached(
        "SELECT region_id, x, y, w, h, kind, reading_order, confidence, review, outline FROM guided_regions
         WHERE comic_id = ?1 AND page_index = ?2 AND source = ?3 ORDER BY reading_order",
    )?;
    let rows = stmt.query_map(params![comic, page, source], |r| {
        Ok(GuidedRegion {
            id: r.get(0)?,
            page_index: page,
            x: r.get(1)?,
            y: r.get(2)?,
            w: r.get(3)?,
            h: r.get(4)?,
            kind: r.get(5)?,
            order: r.get(6)?,
            confidence: r.get(7)?,
            review: r.get(8)?,
            outline: serde_json::from_str(&r.get::<_, String>(9)?).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Painéis da análise automática, se ela ainda valer para o arquivo atual e este detector.
fn current_panels(conn: &Connection, comic: i64, page: i64, fingerprint: &str) -> AppResult<Option<Vec<Rect>>> {
    let panels: Option<String> = conn
        .query_row(
            "SELECT panels FROM guided_pages WHERE comic_id = ?1 AND page_index = ?2 AND fingerprint = ?3 AND detector_version = ?4",
            params![comic, page, fingerprint, DETECTOR_VERSION],
            |r| r.get(0),
        )
        .optional()?;
    Ok(panels.map(|p| serde_json::from_str(&p).unwrap_or_default()))
}

fn sequence(regions: Vec<GuidedRegion>, panels: &[Rect], rtl: bool) -> Vec<GuidedRegion> {
    let rects: Vec<Rect> = regions.iter().map(GuidedRegion::rect).collect();
    reading_sequence(&rects, panels, rtl)
        .iter()
        .enumerate()
        .map(|(n, placed)| GuidedRegion { order: n as i64, ..regions[placed.region].clone() })
        .collect()
}

fn low_confidence(regions: &[GuidedRegion]) -> bool {
    regions.iter().any(|r| r.confidence.is_some_and(|c| c < LOW_CONFIDENCE))
}

/// Lê a página sem analisar nada. `None` significa que falta a análise automática.
/// Correções de outra versão do arquivo não valem mais e deixam de ser usadas.
pub fn read_page(conn: &Connection, comic: i64, page: i64, fingerprint: &str, rtl: bool) -> AppResult<Option<GuidedPage>> {
    let corrected: Option<String> = conn
        .query_row("SELECT fingerprint FROM guided_corrections WHERE comic_id = ?1 AND page_index = ?2", params![comic, page], |r| r.get(0))
        .optional()?;
    if corrected.as_deref() == Some(fingerprint) {
        let panels: Option<String> = conn
            .query_row("SELECT panels FROM guided_pages WHERE comic_id = ?1 AND page_index = ?2", params![comic, page], |r| r.get(0))
            .optional()?;
        let panels = panels.map(|p| serde_json::from_str(&p).unwrap_or_default()).unwrap_or_default();
        return Ok(Some(GuidedPage {
            page_index: page,
            source: "user".into(),
            regions: select_regions(conn, comic, page, "user")?,
            panels,
            needs_review: false,
        }));
    }
    let Some(panels) = current_panels(conn, comic, page, fingerprint)? else {
        return Ok(None);
    };
    let regions = sequence(select_regions(conn, comic, page, "auto")?, &panels, rtl);
    let needs_review = low_confidence(&regions);
    Ok(Some(GuidedPage { page_index: page, source: "auto".into(), regions, panels, needs_review }))
}

/// Grava o resultado automático, substituindo só o resultado automático anterior.
pub fn save_analysis(conn: &mut Connection, comic: i64, page: i64, fingerprint: &str, analysis: &PageAnalysis) -> AppResult<()> {
    let panels = serde_json::to_string(&analysis.panels).map_err(|e| AppError::user("internal", e.to_string()))?;
    let rects: Vec<Rect> = analysis.regions.iter().map(|r| r.rect).collect();
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM guided_regions WHERE comic_id = ?1 AND page_index = ?2 AND source = 'auto'", params![comic, page])?;
    tx.execute(
        "INSERT OR REPLACE INTO guided_pages (comic_id, page_index, fingerprint, detector_version, width, height, panels, analyzed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![comic, page, fingerprint, DETECTOR_VERSION, analysis.width, analysis.height, panels, now_ms()],
    )?;
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO guided_regions (comic_id, page_index, source, region_id, x, y, w, h, kind, reading_order, confidence, review, outline)
             VALUES (?1, ?2, 'auto', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'auto', ?11)",
        )?;
        for (n, placed) in reading_sequence(&rects, &analysis.panels, false).iter().enumerate() {
            let r = &analysis.regions[placed.region];
            insert.execute(params![
                comic,
                page,
                format!("a{page}-{n}"),
                r.rect.x,
                r.rect.y,
                r.rect.w,
                r.rect.h,
                r.kind.as_str(),
                n as i64,
                r.confidence,
                serde_json::to_string(&r.outline).map_err(|e| AppError::user("internal", e.to_string()))?,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Valida o que veio da interface antes de gravar.
fn validate(regions: &[GuidedRegion]) -> AppResult<()> {
    if regions.len() > MAX_REGIONS {
        return Err(AppError::user("invalid", "Há regiões demais nesta página."));
    }
    let mut ids = std::collections::HashSet::new();
    for r in regions {
        let inside = r.x >= -0.001 && r.y >= -0.001 && r.w >= MIN_SIDE && r.h >= MIN_SIDE && r.x + r.w <= 1.001 && r.y + r.h <= 1.001;
        let finite = [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite());
        if !inside || !finite {
            return Err(AppError::user("invalid", "Uma região ficou fora dos limites da página."));
        }
        // O contorno vira estilo na tela: só frações válidas passam.
        if r.outline.len() > MAX_OUTLINE || r.outline.iter().flatten().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(AppError::user("invalid", "O contorno de uma região é inválido."));
        }
        if RegionKind::parse(&r.kind).is_none() {
            return Err(AppError::user("invalid", "Tipo de região desconhecido."));
        }
        if r.id.is_empty() || r.id.len() > 64 || !ids.insert(r.id.as_str()) {
            return Err(AppError::user("invalid", "Identificador de região inválido."));
        }
    }
    Ok(())
}

/// Grava a sequência corrigida. A ordem da lista recebida é a ordem de leitura.
pub fn save_corrections(conn: &mut Connection, comic: i64, page: i64, fingerprint: &str, regions: &[GuidedRegion]) -> AppResult<()> {
    validate(regions)?;
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM guided_regions WHERE comic_id = ?1 AND page_index = ?2 AND source = 'user'", params![comic, page])?;
    tx.execute(
        "INSERT OR REPLACE INTO guided_corrections (comic_id, page_index, fingerprint, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![comic, page, fingerprint, now_ms()],
    )?;
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO guided_regions (comic_id, page_index, source, region_id, x, y, w, h, kind, reading_order, confidence, review, outline)
             VALUES (?1, ?2, 'user', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )?;
        for (n, r) in regions.iter().enumerate() {
            insert.execute(params![
                comic,
                page,
                r.id,
                r.x.max(0.0),
                r.y.max(0.0),
                r.w.min(1.0),
                r.h.min(1.0),
                r.kind,
                n as i64,
                r.confidence,
                if r.review == "auto" { "confirmed" } else { r.review.as_str() },
                serde_json::to_string(&r.outline).map_err(|e| AppError::user("internal", e.to_string()))?,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Descarta as correções da página; a análise automática volta a valer.
pub fn clear_corrections(conn: &mut Connection, comic: i64, page: i64) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM guided_regions WHERE comic_id = ?1 AND page_index = ?2 AND source = 'user'", params![comic, page])?;
    tx.execute("DELETE FROM guided_corrections WHERE comic_id = ?1 AND page_index = ?2", params![comic, page])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::guided::DetectedRegion;

    const FP: &str = "impressao";

    fn conn_with_comic() -> Connection {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO comics (id, path, file_name, format, file_size, fingerprint, title, added_at, updated_at)
             VALUES (1, 'a.cbz', 'a.cbz', 'cbz', 1, ?1, 'A', 1, 1)",
            [FP],
        )
        .unwrap();
        conn
    }

    fn detected(x: f32, y: f32, confidence: f32) -> DetectedRegion {
        DetectedRegion { rect: Rect { x, y, w: 0.2, h: 0.1 }, kind: RegionKind::Speech, confidence, outline: vec![[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]] }
    }

    fn analysis(regions: Vec<DetectedRegion>) -> PageAnalysis {
        PageAnalysis { width: 100, height: 150, panels: Vec::new(), regions }
    }

    fn manual(id: &str, x: f32) -> GuidedRegion {
        GuidedRegion { id: id.into(), page_index: 0, x, y: 0.1, w: 0.2, h: 0.1, kind: "thought".into(), order: 0, confidence: None, review: "added".into(), outline: Vec::new() }
    }

    #[test]
    fn missing_analysis_is_not_invented() {
        let conn = conn_with_comic();
        assert!(read_page(&conn, 1, 0, FP, false).unwrap().is_none());
    }

    #[test]
    fn automatic_order_follows_the_direction_requested_at_read_time() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9), detected(0.6, 0.1, 0.9)])).unwrap();
        let ltr = read_page(&conn, 1, 0, FP, false).unwrap().unwrap();
        let rtl = read_page(&conn, 1, 0, FP, true).unwrap().unwrap();
        assert!(ltr.regions[0].x < ltr.regions[1].x);
        assert!(rtl.regions[0].x > rtl.regions[1].x);
        assert_eq!(rtl.regions[0].order, 0);
    }

    #[test]
    fn low_confidence_asks_for_review_until_corrected() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.4)])).unwrap();
        let page = read_page(&conn, 1, 0, FP, false).unwrap().unwrap();
        assert!(page.needs_review);
        save_corrections(&mut conn, 1, 0, FP, &page.regions).unwrap();
        let fixed = read_page(&conn, 1, 0, FP, false).unwrap().unwrap();
        assert!(!fixed.needs_review);
        assert_eq!(fixed.regions[0].review, "confirmed");
        assert_eq!(fixed.regions[0].outline, page.regions[0].outline);
    }

    #[test]
    fn corrections_survive_a_new_analysis_and_keep_their_order() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9)])).unwrap();
        save_corrections(&mut conn, 1, 0, FP, &[manual("u2", 0.7), manual("u1", 0.1)]).unwrap();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.3, 0.3, 0.9), detected(0.5, 0.5, 0.9)])).unwrap();
        let page = read_page(&conn, 1, 0, FP, false).unwrap().unwrap();
        assert_eq!(page.source, "user");
        assert_eq!(page.regions.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["u2", "u1"]);
        assert_eq!(page.regions[0].kind, "thought");
    }

    #[test]
    fn page_confirmed_as_empty_stays_empty() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9)])).unwrap();
        save_corrections(&mut conn, 1, 0, FP, &[]).unwrap();
        let page = read_page(&conn, 1, 0, FP, false).unwrap().unwrap();
        assert_eq!(page.source, "user");
        assert!(page.regions.is_empty());
    }

    #[test]
    fn clearing_corrections_restores_the_automatic_result() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9)])).unwrap();
        save_corrections(&mut conn, 1, 0, FP, &[manual("u1", 0.5)]).unwrap();
        clear_corrections(&mut conn, 1, 0).unwrap();
        let page = read_page(&conn, 1, 0, FP, false).unwrap().unwrap();
        assert_eq!(page.source, "auto");
        assert_eq!(page.regions.len(), 1);
    }

    #[test]
    fn changed_file_invalidates_analysis_and_corrections() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9)])).unwrap();
        save_corrections(&mut conn, 1, 0, FP, &[manual("u1", 0.5)]).unwrap();
        assert!(read_page(&conn, 1, 0, "outro-arquivo", false).unwrap().is_none());
    }

    #[test]
    fn older_detector_results_are_redone_but_corrections_stay() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9)])).unwrap();
        conn.execute("UPDATE guided_pages SET detector_version = detector_version - 1", []).unwrap();
        assert!(read_page(&conn, 1, 0, FP, false).unwrap().is_none());
        save_corrections(&mut conn, 1, 0, FP, &[manual("u1", 0.5)]).unwrap();
        assert_eq!(read_page(&conn, 1, 0, FP, false).unwrap().unwrap().source, "user");
    }

    #[test]
    fn invalid_corrections_are_refused() {
        let mut conn = conn_with_comic();
        let mut out = manual("u1", 0.9);
        out.w = 0.5;
        assert!(save_corrections(&mut conn, 1, 0, FP, &[out]).is_err());
        let mut kind = manual("u1", 0.1);
        kind.kind = "explosao".into();
        assert!(save_corrections(&mut conn, 1, 0, FP, &[kind]).is_err());
        assert!(save_corrections(&mut conn, 1, 0, FP, &[manual("u1", 0.1), manual("u1", 0.4)]).is_err());
        let mut outline = manual("u1", 0.1);
        outline.outline = vec![[0.5, f32::NAN]];
        assert!(save_corrections(&mut conn, 1, 0, FP, &[outline]).is_err());
    }

    #[test]
    fn removing_the_comic_removes_its_regions() {
        let mut conn = conn_with_comic();
        save_analysis(&mut conn, 1, 0, FP, &analysis(vec![detected(0.1, 0.1, 0.9)])).unwrap();
        save_corrections(&mut conn, 1, 0, FP, &[manual("u1", 0.5)]).unwrap();
        conn.execute("DELETE FROM comics WHERE id = 1", []).unwrap();
        let left: i64 = conn.query_row("SELECT count(*) FROM guided_regions", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }
}
