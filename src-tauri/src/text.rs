//! Utilidades de texto: normalização para busca e título a partir do nome do arquivo.

use unicode_normalization::UnicodeNormalization;

/// Minúsculas e sem acentos, para busca tolerante ("Ação" encontra "acao").
pub fn fold(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Deriva um título legível do nome do arquivo (sem extensão).
pub fn title_from_file_name(name: &str, is_dir: bool) -> String {
    let stem = if is_dir {
        name
    } else {
        match name.rfind('.') {
            Some(i) if i > 0 => &name[..i],
            _ => name,
        }
    };
    let replaced: String = stem.chars().map(|c| if c == '_' || c == '.' { ' ' } else { c }).collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() { name.to_string() } else { collapsed }
}

/// Remove espaços e devolve `None` para strings vazias.
pub fn clean_opt(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_removes_accents_and_case() {
        assert_eq!(fold("Ação PÁGINA"), "acao pagina");
    }

    #[test]
    fn titles_from_file_names() {
        assert_eq!(title_from_file_name("Minha_HQ_v01.cbz", false), "Minha HQ v01");
        assert_eq!(title_from_file_name("Turma.da.Monica.042.cbr", false), "Turma da Monica 042");
        assert_eq!(title_from_file_name("Pasta.Com.Pontos", true), "Pasta Com Pontos");
        assert_eq!(title_from_file_name(".cbz", false), "cbz");
    }
}
