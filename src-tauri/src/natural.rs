//! Ordenação natural: números dentro dos nomes são comparados pelo valor.
//! `1.jpg, 2.jpg, 10.jpg` em vez de `1.jpg, 10.jpg, 2.jpg`.

use std::cmp::Ordering;

#[derive(Debug, PartialEq, Eq)]
enum Chunk<'a> {
    Digits(&'a str),
    Text(&'a str),
}

fn chunks(s: &str) -> Vec<Chunk<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_digits: Option<bool> = None;
    for (i, c) in s.char_indices() {
        let is_digit = c.is_ascii_digit();
        match in_digits {
            Some(prev) if prev != is_digit => {
                let part = &s[start..i];
                out.push(if prev { Chunk::Digits(part) } else { Chunk::Text(part) });
                start = i;
            }
            _ => {}
        }
        in_digits = Some(is_digit);
    }
    if start < s.len() {
        let part = &s[start..];
        out.push(if in_digits == Some(true) { Chunk::Digits(part) } else { Chunk::Text(part) });
    }
    out
}

fn cmp_digits(a: &str, b: &str) -> Ordering {
    let ta = a.trim_start_matches('0');
    let tb = b.trim_start_matches('0');
    ta.len()
        .cmp(&tb.len())
        .then_with(|| ta.cmp(tb))
        // "01" depois de "1" para manter a ordem total e estável.
        .then_with(|| a.len().cmp(&b.len()))
}

/// Compara dois nomes (sem separadores de diretório) de forma natural e sem diferenciar maiúsculas.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let ca = chunks(a);
    let cb = chunks(b);
    for (x, y) in ca.iter().zip(cb.iter()) {
        let ord = match (x, y) {
            (Chunk::Digits(x), Chunk::Digits(y)) => cmp_digits(x, y),
            (Chunk::Text(x), Chunk::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
            (Chunk::Digits(_), Chunk::Text(_)) => Ordering::Less,
            (Chunk::Text(_), Chunk::Digits(_)) => Ordering::Greater,
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    ca.len().cmp(&cb.len()).then_with(|| a.cmp(b))
}

/// Compara caminhos relativos componente a componente (pastas primeiro pela ordem natural).
pub fn natural_path_cmp(a: &str, b: &str) -> Ordering {
    let pa: Vec<&str> = a.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let pb: Vec<&str> = b.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    for (x, y) in pa.iter().zip(pb.iter()) {
        let ord = natural_cmp(x, y);
        if ord != Ordering::Equal {
            return ord;
        }
    }
    pa.len().cmp(&pb.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| natural_path_cmp(a, b));
        v
    }

    #[test]
    fn numbers_compare_by_value() {
        assert_eq!(
            sorted(vec!["10.jpg", "2.jpg", "1.jpg", "3.jpg"]),
            vec!["1.jpg", "2.jpg", "3.jpg", "10.jpg"]
        );
    }

    #[test]
    fn leading_zeros_and_prefixes() {
        assert_eq!(
            sorted(vec!["page_010.png", "page_002.png", "page_1.png", "Page_003.png"]),
            vec!["page_1.png", "page_002.png", "Page_003.png", "page_010.png"]
        );
    }

    #[test]
    fn directories_sort_naturally_first() {
        assert_eq!(
            sorted(vec!["cap10/01.jpg", "cap2/02.jpg", "cap2/01.jpg", "cap1/10.jpg", "cap1/9.jpg"]),
            vec!["cap1/9.jpg", "cap1/10.jpg", "cap2/01.jpg", "cap2/02.jpg", "cap10/01.jpg"]
        );
    }

    #[test]
    fn case_insensitive_and_total_order() {
        assert_eq!(natural_cmp("a", "A"), "a".cmp("A"));
        assert_eq!(natural_cmp("abc", "ABD"), Ordering::Less);
        assert_eq!(natural_cmp("1", "01"), Ordering::Less);
        assert_eq!(natural_cmp("x", "x"), Ordering::Equal);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(
            natural_cmp("p99999999999999999999999.jpg", "p100000000000000000000000.jpg"),
            Ordering::Less
        );
    }
}
