//! Detecção e ordem da Leitura Guiada em páginas reais de quadrinhos.
//!
//! Os quadrinhos e o gabarito (`tests/real/manifest.json`) ficam só nesta máquina, fora do git:
//! sem eles estes testes falham de propósito, para ninguém achar que a detecção foi conferida.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use prancha_lib::covers::decode_limited;
use prancha_lib::formats;
use prancha_lib::guided::order::reading_sequence;
use prancha_lib::guided::store::{read_page, save_analysis, save_corrections};
use prancha_lib::guided::{PageAnalysis, Rect, analyze};
use serde::Deserialize;

/// Tamanho da imagem em que o gabarito foi marcado.
const MARKED: (f32, f32) = (715.0, 1100.0);
const LOCAL_MANIFEST: &str = "../tests/real/manifest.json";

type Point = [f32; 2];

#[derive(Deserialize)]
struct Manifest {
    comics: Vec<Comic>,
}

#[derive(Deserialize)]
struct Comic {
    path: String,
    cases: Vec<Case>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Case {
    page: usize,
    note: String,
    expect: Vec<Point>,
    extras: usize,
    same_region: Vec<[Point; 2]>,
    rtl: Option<Vec<usize>>,
    doubtful: Vec<Point>,
    spread: bool,
}

struct Page {
    name: String,
    case: Case,
    analysis: PageAnalysis,
}

fn analyzed_pages() -> &'static Vec<Page> {
    static PAGES: OnceLock<Vec<Page>> = OnceLock::new();
    PAGES.get_or_init(|| {
        let text = std::fs::read_to_string(LOCAL_MANIFEST).unwrap_or_else(|e| panic!("faltam as páginas reais em {LOCAL_MANIFEST}: {e}"));
        let manifest: Manifest = serde_json::from_str(&text).unwrap();
        let mut out = Vec::new();
        for comic in manifest.comics {
            let path = Path::new(&comic.path);
            let format = formats::detect(path).unwrap();
            let listing = formats::list(format, path).unwrap();
            for case in comic.cases {
                let bytes = formats::read_entry(format, path, &listing.pages[case.page]).unwrap();
                let image = decode_limited(&bytes).unwrap();
                let start = Instant::now();
                let analysis = analyze(&image);
                let name = format!("{} p{} ({})", path.file_stem().unwrap().to_string_lossy(), case.page, case.note);
                println!("{name}: {} regiões em {} ms", analysis.regions.len(), start.elapsed().as_millis());
                out.push(Page { name, case, analysis });
            }
        }
        out
    })
}

fn at(p: Point, spread: bool) -> (f32, f32) {
    let width = if spread { MARKED.0 * 2.0 } else { MARKED.0 };
    (p[0] / width, p[1] / MARKED.1)
}

fn covers(r: &Rect, p: (f32, f32)) -> bool {
    p.0 >= r.x && p.0 <= r.right() && p.1 >= r.y && p.1 <= r.bottom()
}

fn center_distance(r: &Rect, p: (f32, f32)) -> f32 {
    (r.x + r.w / 2.0 - p.0).hypot(r.y + r.h / 2.0 - p.1)
}

/// Lê as regiões na ordem pedida e devolve o balão do gabarito de cada uma.
/// Regiões que não cobrem nenhum balão esperado voltam como `None`.
fn sequence_against_expected(page: &Page, rtl: bool) -> Vec<Option<usize>> {
    let a = &page.analysis;
    let rects: Vec<Rect> = a.regions.iter().map(|r| r.rect).collect();
    let points: Vec<(f32, f32)> = page.case.expect.iter().map(|p| at(*p, page.case.spread)).collect();
    reading_sequence(&rects, &a.panels, rtl)
        .iter()
        .map(|placed| {
            let r = &rects[placed.region];
            // Balões vizinhos podem se sobrepor: vale o centro mais próximo entre os cobertos.
            (0..points.len()).filter(|i| covers(r, points[*i])).min_by(|x, y| center_distance(r, points[*x]).total_cmp(&center_distance(r, points[*y])))
        })
        .collect()
}

#[test]
fn every_balloon_is_found_once_in_reading_order() {
    let mut failures = Vec::new();
    for page in analyzed_pages() {
        let got = sequence_against_expected(page, false);
        let matched: Vec<usize> = got.iter().flatten().copied().collect();
        let wanted: Vec<usize> = (0..page.case.expect.len()).collect();
        let extras = got.iter().filter(|m| m.is_none()).count();
        if matched != wanted || extras != page.case.extras {
            failures.push(format!("{}: ordem {matched:?} em vez de {wanted:?}; {extras} a mais (esperado {})", page.name, page.case.extras));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn every_line_of_a_balloon_stays_in_the_same_region() {
    for page in analyzed_pages() {
        for [a, b] in &page.case.same_region {
            let (a, b) = (at(*a, page.case.spread), at(*b, page.case.spread));
            let together = page.analysis.regions.iter().any(|r| covers(&r.rect, a) && covers(&r.rect, b));
            assert!(together, "{}: linhas {a:?} e {b:?} ficaram separadas", page.name);
        }
    }
}

#[test]
fn right_to_left_order_reverses_panels_and_balloons() {
    let mut checked = 0;
    for page in analyzed_pages() {
        if let Some(rtl) = &page.case.rtl {
            let got: Vec<usize> = sequence_against_expected(page, true).into_iter().flatten().collect();
            assert_eq!(&got, rtl, "{}", page.name);
            checked += 1;
        }
    }
    assert!(checked > 0, "o gabarito não tem nenhum caso de direita para a esquerda");
}

#[test]
fn balloons_on_doubtful_ground_are_flagged_for_review() {
    let mut checked = 0;
    for page in analyzed_pages() {
        for p in &page.case.doubtful {
            let p = at(*p, page.case.spread);
            let region = page.analysis.regions.iter().find(|r| covers(&r.rect, p)).unwrap_or_else(|| panic!("{}: região não encontrada", page.name));
            assert!(region.confidence < prancha_lib::guided::LOW_CONFIDENCE, "{}: confiança {}", page.name, region.confidence);
            checked += 1;
        }
    }
    assert!(checked > 0, "o gabarito não tem região duvidosa");
}

#[test]
fn balloon_outlines_stay_inside_the_region_and_reach_its_edges() {
    let mut shaped = 0;
    for page in analyzed_pages() {
        for r in &page.analysis.regions {
            if r.outline.is_empty() {
                continue;
            }
            shaped += 1;
            assert!(r.outline.len() >= 4, "{}: contorno com {} pontos", page.name, r.outline.len());
            assert!(r.outline.iter().flatten().all(|v| (0.0..=1.0).contains(v)), "{}: contorno fora da região", page.name);
            let (xs, ys): (Vec<f32>, Vec<f32>) = r.outline.iter().map(|p| (p[0], p[1])).unzip();
            let spread = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max) - v.iter().copied().fold(f32::MAX, f32::min);
            assert!(spread(&xs) > 0.8 && spread(&ys) > 0.8, "{}: contorno menor que a região ({} x {})", page.name, spread(&xs), spread(&ys));
        }
    }
    assert!(shaped >= 50, "só {shaped} balões com contorno");
}

#[test]
fn real_analysis_is_stored_and_corrections_replace_it() {
    let page = analyzed_pages().iter().find(|p| !p.case.doubtful.is_empty()).unwrap();
    let mut conn = prancha_lib::db::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO comics (id, path, file_name, format, file_size, fingerprint, title, added_at, updated_at) VALUES (1, 'a.cbr', 'a.cbr', 'cbr', 1, 'fp', 'A', 1, 1)",
        [],
    )
    .unwrap();
    save_analysis(&mut conn, 1, 23, "fp", &page.analysis).unwrap();
    let stored = read_page(&conn, 1, 23, "fp", false).unwrap().unwrap();
    assert_eq!(stored.regions.len(), page.analysis.regions.len());
    assert!(stored.regions.iter().any(|r| !r.outline.is_empty()), "o contorno não voltou do banco");
    assert!(stored.needs_review);
    let ids: HashSet<&str> = stored.regions.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), stored.regions.len());
    save_corrections(&mut conn, 1, 23, "fp", &stored.regions[1..]).unwrap();
    let corrected = read_page(&conn, 1, 23, "fp", false).unwrap().unwrap();
    assert_eq!(corrected.source, "user");
    assert_eq!(corrected.regions.len(), stored.regions.len() - 1);
    assert!(!corrected.needs_review);
}
