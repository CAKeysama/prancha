//! Ordem de leitura: primeiro os quadros, depois as regiões dentro de cada quadro.
//!
//! Corta o conjunto de retângulos recursivamente: faixas de cima para baixo, depois colunas
//! no sentido da leitura. Ordenar só por x e y falha em páginas com quadros de alturas diferentes.

use super::Rect;

/// Sobreposição tolerada entre grupos, em fração da altura ou largura do menor retângulo.
const OVERLAP_TOLERANCE: f32 = 0.2;
/// Peso da posição horizontal quando nenhum corte separa os retângulos.
const STAGGER_WEIGHT: f32 = 0.5;
/// Fração mínima da região dentro de um quadro para pertencer a ele.
const PANEL_MEMBERSHIP: f32 = 0.25;

#[derive(Clone, Copy)]
enum Axis {
    Vertical,
    Horizontal,
}

fn span(r: &Rect, axis: Axis) -> (f32, f32) {
    match axis {
        Axis::Vertical => (r.y, r.bottom()),
        Axis::Horizontal => (r.x, r.right()),
    }
}

/// Divide em grupos separados ao longo do eixo.
fn split(items: &[usize], rects: &[Rect], axis: Axis) -> Vec<Vec<usize>> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| span(&rects[*a], axis).0.total_cmp(&span(&rects[*b], axis).0));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut end = f32::MIN;
    let mut smallest = f32::MAX;
    for i in sorted {
        let (s, e) = span(&rects[i], axis);
        let len = e - s;
        let tolerance = OVERLAP_TOLERANCE * smallest.min(len);
        if groups.is_empty() || s >= end - tolerance {
            groups.push(vec![i]);
            end = e;
            smallest = len;
        } else {
            groups.last_mut().unwrap().push(i);
            end = end.max(e);
            smallest = smallest.min(len);
        }
    }
    groups
}

fn walk(items: &[usize], rects: &[Rect], rtl: bool, out: &mut Vec<usize>) {
    if items.len() <= 1 {
        out.extend_from_slice(items);
        return;
    }
    let rows = split(items, rects, Axis::Vertical);
    if rows.len() > 1 {
        for row in &rows {
            walk(row, rects, rtl, out);
        }
        return;
    }
    let mut cols = split(items, rects, Axis::Horizontal);
    if cols.len() > 1 {
        if rtl {
            cols.reverse();
        }
        for col in &cols {
            walk(col, rects, rtl, out);
        }
        return;
    }
    // Sobreposição nos dois eixos: lê primeiro o mais alto e mais próximo do início da linha.
    let score = |r: &Rect| {
        let along = if rtl { 1.0 - r.right() } else { r.x };
        r.y + along * STAGGER_WEIGHT
    };
    let first = *items.iter().min_by(|a, b| score(&rects[**a]).total_cmp(&score(&rects[**b]))).unwrap();
    out.push(first);
    let rest: Vec<usize> = items.iter().copied().filter(|i| *i != first).collect();
    walk(&rest, rects, rtl, out);
}

/// Índices dos retângulos na ordem de leitura.
pub fn order_rects(rects: &[Rect], rtl: bool) -> Vec<usize> {
    let items: Vec<usize> = (0..rects.len()).collect();
    let mut out = Vec::with_capacity(rects.len());
    walk(&items, rects, rtl, &mut out);
    out
}

/// Uma região na sequência final, com o quadro a que pertence (se houver).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub region: usize,
    pub panel: Option<usize>,
}

/// Ordem final: quadros na ordem de leitura e, em cada um, suas regiões.
/// Quem atravessa quadros entra no primeiro lido; quem fica fora de todos ocupa sua posição na página.
pub fn reading_sequence(regions: &[Rect], panels: &[Rect], rtl: bool) -> Vec<Placed> {
    let panel_order = order_rects(panels, rtl);
    let rank = |p: usize| panel_order.iter().position(|q| *q == p).unwrap_or(usize::MAX);
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); panels.len()];
    let mut loose: Vec<usize> = Vec::new();
    for (i, r) in regions.iter().enumerate() {
        let home = (0..panels.len())
            .filter(|p| r.intersection(&panels[*p]) >= PANEL_MEMBERSHIP * r.area().max(f32::EPSILON))
            .min_by_key(|p| rank(*p));
        match home {
            Some(p) => members[p].push(i),
            None => loose.push(i),
        }
    }
    // Quadros com regiões e regiões soltas disputam a ordem como blocos da página.
    let mut blocks: Vec<(Rect, Option<usize>, Vec<usize>)> = Vec::new();
    for (p, list) in members.iter().enumerate() {
        if !list.is_empty() {
            blocks.push((panels[p], Some(p), list.clone()));
        }
    }
    for i in loose {
        blocks.push((regions[i], None, vec![i]));
    }
    let block_rects: Vec<Rect> = blocks.iter().map(|b| b.0).collect();
    let mut out = Vec::with_capacity(regions.len());
    for b in order_rects(&block_rects, rtl) {
        let (_, panel, list) = &blocks[b];
        let rects: Vec<Rect> = list.iter().map(|i| regions[*i]).collect();
        for k in order_rects(&rects, rtl) {
            out.push(Placed { region: list[k], panel: *panel });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn grid_reads_in_rows_following_direction() {
        let grid = [r(0.0, 0.0, 0.4, 0.4), r(0.5, 0.0, 0.4, 0.4), r(0.0, 0.5, 0.4, 0.4), r(0.5, 0.5, 0.4, 0.4)];
        assert_eq!(order_rects(&grid, false), vec![0, 1, 2, 3]);
        assert_eq!(order_rects(&grid, true), vec![1, 0, 3, 2]);
    }

    #[test]
    fn tall_panel_is_read_before_the_stacked_column() {
        // Quadro alto à esquerda, dois empilhados à direita: ordenar só por y erraria.
        let page = [r(0.5, 0.0, 0.45, 0.45), r(0.0, 0.0, 0.45, 1.0), r(0.5, 0.5, 0.45, 0.5)];
        assert_eq!(order_rects(&page, false), vec![1, 0, 2]);
        assert_eq!(order_rects(&page, true), vec![0, 2, 1]);
    }

    #[test]
    fn irregular_layout_keeps_strips_together() {
        // Faixa larga, depois coluna com quadro alto + dois menores, depois outra faixa.
        let page = [
            r(0.0, 0.0, 1.0, 0.2),
            r(0.55, 0.25, 0.45, 0.2),
            r(0.0, 0.25, 0.5, 0.45),
            r(0.55, 0.5, 0.45, 0.2),
            r(0.0, 0.75, 1.0, 0.25),
        ];
        assert_eq!(order_rects(&page, false), vec![0, 2, 1, 3, 4]);
        assert_eq!(order_rects(&page, true), vec![0, 1, 3, 2, 4]);
    }

    #[test]
    fn small_overlap_does_not_join_rows() {
        let page = [r(0.5, 0.42, 0.4, 0.4), r(0.0, 0.0, 0.4, 0.44)];
        assert_eq!(order_rects(&page, false), vec![1, 0]);
    }

    #[test]
    fn overlapping_on_both_axes_starts_from_the_top_of_the_line() {
        let page = [r(0.3, 0.3, 0.4, 0.4), r(0.0, 0.0, 0.5, 0.5), r(0.5, 0.5, 0.5, 0.5)];
        assert_eq!(order_rects(&page, false), vec![1, 0, 2]);
    }

    #[test]
    fn regions_follow_their_panel_not_the_page_position() {
        let panels = [r(0.0, 0.0, 0.45, 1.0), r(0.5, 0.0, 0.5, 0.45), r(0.5, 0.5, 0.5, 0.5)];
        // Balão baixo no quadro alto vem antes do balão no topo do quadro da direita.
        let regions = [r(0.6, 0.05, 0.2, 0.1), r(0.1, 0.8, 0.2, 0.1), r(0.1, 0.1, 0.2, 0.1), r(0.6, 0.6, 0.2, 0.1)];
        let seq: Vec<usize> = reading_sequence(&regions, &panels, false).iter().map(|p| p.region).collect();
        assert_eq!(seq, vec![2, 1, 0, 3]);
        let rtl: Vec<usize> = reading_sequence(&regions, &panels, true).iter().map(|p| p.region).collect();
        assert_eq!(rtl, vec![0, 3, 2, 1]);
    }

    #[test]
    fn balloon_crossing_panels_stays_with_the_first_read() {
        let panels = [r(0.0, 0.0, 0.48, 0.5), r(0.52, 0.0, 0.48, 0.5)];
        let regions = [r(0.6, 0.1, 0.2, 0.1), r(0.4, 0.3, 0.2, 0.1)];
        let seq = reading_sequence(&regions, &panels, false);
        assert_eq!(seq[0], Placed { region: 1, panel: Some(0) });
        assert_eq!(seq[1], Placed { region: 0, panel: Some(1) });
        let rtl = reading_sequence(&regions, &panels, true);
        assert_eq!(rtl[0].panel, Some(1));
        assert_eq!(rtl.len(), 2);
    }

    #[test]
    fn regions_outside_every_panel_are_not_lost() {
        let panels = [r(0.0, 0.3, 1.0, 0.3)];
        let regions = [r(0.1, 0.4, 0.2, 0.1), r(0.1, 0.02, 0.5, 0.1), r(0.1, 0.8, 0.5, 0.1)];
        let seq: Vec<usize> = reading_sequence(&regions, &panels, false).iter().map(|p| p.region).collect();
        assert_eq!(seq, vec![1, 0, 2]);
    }

    #[test]
    fn page_without_panels_orders_the_balloons() {
        let regions = [r(0.6, 0.1, 0.2, 0.1), r(0.1, 0.1, 0.2, 0.1)];
        let seq: Vec<usize> = reading_sequence(&regions, &[], false).iter().map(|p| p.region).collect();
        assert_eq!(seq, vec![1, 0]);
        assert!(reading_sequence(&[], &[], false).is_empty());
    }
}
