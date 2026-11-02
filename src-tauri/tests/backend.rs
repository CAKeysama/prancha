//! Testes de integração do backend: formatos, importação, capas, catálogo e progresso.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use common::*;
use prancha_lib::covers::COVER_PARAMS_VERSION;
use prancha_lib::formats::{self, ComicFormat};
use prancha_lib::import::{self, DuplicatePolicy, ItemResult};
use prancha_lib::library::{self, LibraryQuery, MetadataPatch};
use prancha_lib::pages::PageService;
use prancha_lib::db;
use rusqlite::Connection;

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    covers: PathBuf,
    db_path: PathBuf,
    conn: Connection,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let covers = root.join("covers");
    std::fs::create_dir_all(&covers).unwrap();
    std::fs::create_dir_all(root.join("in")).unwrap();
    let db_path = root.join("t.db");
    let conn = db::open(&db_path).unwrap();
    Env { _dir: dir, covers, conn, db_path, root }
}

impl Env {
    fn input(&self, name: &str) -> PathBuf {
        self.root.join("in").join(name)
    }
    fn import(&self, path: &Path) -> ItemResult {
        import::process_one(&self.conn, &self.covers, path, DuplicatePolicy::Skip)
    }
    fn import_ok(&self, path: &Path) -> i64 {
        let r = self.import(path);
        assert_eq!(r.status, "added", "import falhou: {r:?}");
        r.comic_id.unwrap()
    }
}

// ---------------------------------------------------------------------------
// Formatos
// ---------------------------------------------------------------------------

#[test]
fn cbz_pages_are_naturally_ordered_and_filtered() {
    let e = env();
    let files = vec![
        ("10.png", png(10, 10, 1)),
        ("2.png", png(10, 10, 2)),
        ("1.png", png(10, 10, 3)),
        ("__MACOSX/._1.png", vec![0; 10]),
        ("notas.txt", b"x".to_vec()),
        ("3.jpg", jpeg(10, 10, 4)),
    ];
    let p = make_cbz(&e.input("ordem.cbz"), &files);
    let listing = formats::list(ComicFormat::Cbz, &p).unwrap();
    let names: Vec<&str> = listing.pages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["1.png", "2.png", "3.jpg", "10.png"]);
}

#[test]
fn detect_uses_content_signature() {
    let e = env();
    // .cbr que na verdade é ZIP: comum em arquivos reais.
    let p = make_cbz(&e.input("falso.cbr"), &as_files(&pages(2)));
    assert_eq!(formats::detect(&p).unwrap(), ComicFormat::Cbz);
    let rar = make_rar5(&e.input("a.cbr"), &as_files(&pages(1)));
    assert_eq!(formats::detect(&rar).unwrap(), ComicFormat::Cbr);
    let pdf = make_pdf(&e.input("a.pdf"), 1, None);
    assert_eq!(formats::detect(&pdf).unwrap(), ComicFormat::Pdf);
    let cb7 = make_cb7(&e.input("a.cb7"), &as_files(&pages(1)));
    assert_eq!(formats::detect(&cb7).unwrap(), ComicFormat::Cb7);
    let cbt = make_cbt(&e.input("a.cbt"), &as_files(&pages(1)));
    assert_eq!(formats::detect(&cbt).unwrap(), ComicFormat::Cbt);
}

#[test]
fn rar5_listing_reading_and_extraction() {
    let e = env();
    let pg = pages(12);
    let p = make_rar5(&e.input("serie.cbr"), &as_files(&pg));
    let listing = formats::list(ComicFormat::Cbr, &p).unwrap();
    assert_eq!(listing.pages.len(), 12);
    assert_eq!(listing.pages[9].name, "10.png");
    let data = formats::read_entry(ComicFormat::Cbr, &p, &listing.pages[9]).unwrap();
    assert_eq!(data, pg[9].1);
    let wanted = listing.pages.iter().enumerate().map(|(i, p)| (p.name.clone(), i)).collect();
    let mut got = vec![None; 12];
    formats::extract_pages(ComicFormat::Cbr, &p, &wanted, &mut |i, d| {
        got[i] = Some(d);
        Ok(true)
    })
    .unwrap();
    for (i, d) in got.into_iter().enumerate() {
        assert_eq!(d.unwrap(), pg[i].1, "página {i}");
    }
}

#[test]
fn cb7_and_cbt_round_trip() {
    let e = env();
    let pg = pages(5);
    let p7 = make_cb7(&e.input("x.cb7"), &as_files(&pg));
    let l7 = formats::list(ComicFormat::Cb7, &p7).unwrap();
    assert_eq!(l7.pages.len(), 5);
    assert_eq!(formats::read_entry(ComicFormat::Cb7, &p7, &l7.pages[3]).unwrap(), pg[3].1);
    let wanted = l7.pages.iter().enumerate().map(|(i, p)| (p.name.clone(), i)).collect();
    let mut count = 0;
    formats::extract_pages(ComicFormat::Cb7, &p7, &wanted, &mut |i, d| {
        assert_eq!(d, pg[i].1);
        count += 1;
        Ok(true)
    })
    .unwrap();
    assert_eq!(count, 5);

    let pt = make_cbt(&e.input("x.cbt"), &as_files(&pg));
    let lt = formats::list(ComicFormat::Cbt, &pt).unwrap();
    assert_eq!(lt.pages.len(), 5);
    assert_eq!(formats::read_entry(ComicFormat::Cbt, &pt, &lt.pages[4]).unwrap(), pg[4].1);
}

#[test]
fn pdf_metadata_and_page_count() {
    let e = env();
    let p = make_pdf(&e.input("doc.pdf"), 7, Some("Minha Graphic Novel"));
    let l = formats::list(ComicFormat::Pdf, &p).unwrap();
    assert_eq!(l.page_count, 7);
    assert_eq!(l.pdf_title.as_deref(), Some("Minha Graphic Novel"));
    assert_eq!(l.pdf_author.as_deref(), Some("Autora Teste"));
}

#[test]
fn comicinfo_inside_archives() {
    let e = env();
    let xml = r#"<?xml version="1.0"?><ComicInfo><Title>Capítulo Final</Title><Series>Série X</Series><Number>7</Number>
        <Writer>Fulana</Writer><Tags>drama</Tags><Pages><Page Image="2" Type="FrontCover"/></Pages></ComicInfo>"#;
    let mut files = as_files(&pages(4));
    files.push(("ComicInfo.xml", xml.as_bytes().to_vec()));
    for (name, maker) in [
        ("m.cbz", make_cbz as fn(&Path, Files) -> PathBuf),
        ("m.cbr", make_rar5),
        ("m.cb7", make_cb7),
        ("m.cbt", make_cbt),
    ] {
        let p = maker(&e.input(name), &files);
        let f = formats::detect(&p).unwrap();
        let l = formats::list(f, &p).unwrap();
        let info = l.comic_info.unwrap_or_else(|| panic!("sem ComicInfo em {name}"));
        assert_eq!(info.title.as_deref(), Some("Capítulo Final"), "{name}");
        assert_eq!(info.front_cover, Some(2), "{name}");
        assert_eq!(l.pages.len(), 4, "{name}");
    }
}

#[test]
fn corrupt_and_unsupported_files_are_reported() {
    let e = env();
    let bad = e.input("quebrado.cbz");
    std::fs::write(&bad, b"PK\x03\x04 isto nao e um zip de verdade").unwrap();
    let r = e.import(&bad);
    assert_eq!(r.status, "failed");
    assert!(r.message.unwrap().contains("corrompido"), "mensagem deve explicar");

    let random = e.input("aleatorio.cbr");
    std::fs::write(&random, vec![7u8; 4096]).unwrap();
    let r = e.import(&random);
    assert_eq!(r.status, "failed");
    assert!(r.message.unwrap().contains("cabeçalho inválido"));

    let empty = e.input("vazio.cbz");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(e.import(&empty).status, "failed");

    let txt = e.input("leia.txt");
    std::fs::write(&txt, b"oi").unwrap();
    let (cands, errors) = import::expand(&[txt]);
    assert!(cands.is_empty());
    assert!(errors[0].message.as_ref().unwrap().contains("Formato não suportado"));

    let no_images = make_cbz(&e.input("sem-imagens.cbz"), &[("a.txt", b"x".to_vec())]);
    let r = e.import(&no_images);
    assert_eq!(r.status, "failed");
    assert!(r.message.unwrap().contains("Nenhuma página"));

    // RAR cortado antes da primeira entrada completa.
    let truncated = e.input("cortado.cbr");
    let full = std::fs::read(make_rar5(&e.input("inteiro.cbr"), &as_files(&pages(3)))).unwrap();
    std::fs::write(&truncated, &full[..40]).unwrap();
    let r = e.import(&truncated);
    assert_eq!(r.status, "failed", "{r:?}");
}

#[test]
fn damaged_page_is_reported_without_breaking_the_comic() {
    let e = env();
    let pg = pages(3);
    let p = e.input("pagina-danificada.cbz");
    {
        let f = std::fs::File::create(&p).unwrap();
        let mut zip = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in &pg {
            zip.start_file(*name, opts).unwrap();
            std::io::Write::write_all(&mut zip, data).unwrap();
        }
        zip.finish().unwrap();
    }
    // Corrompe bytes no meio da segunda página (o CRC deixa de bater).
    let mut bytes = std::fs::read(&p).unwrap();
    let page = &pg[1].1;
    let start = bytes.windows(page.len()).position(|w| w == page.as_slice()).unwrap();
    let pos = start + page.len() / 2;
    for b in &mut bytes[pos..pos + 16] {
        *b = !*b;
    }
    std::fs::write(&p, bytes).unwrap();
    let id = e.import_ok(&p);
    let info = library::file_info(&e.conn, id).unwrap();
    let svc = PageService::new(e.root.join("pc"));
    assert!(svc.read_page(&info, 0).is_ok());
    assert!(svc.read_page(&info, 1).is_err(), "página danificada gera erro tratável");
    assert!(svc.read_page(&info, 2).is_ok(), "as demais páginas continuam legíveis");
}

#[cfg(windows)]
#[test]
fn locked_file_fails_gracefully() {
    use std::os::windows::fs::OpenOptionsExt;
    let e = env();
    let p = make_cbz(&e.input("bloqueado.cbz"), &as_files(&pages(2)));
    let _lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&p).unwrap();
    let r = e.import(&p);
    assert_eq!(r.status, "failed");
    assert!(r.message.is_some());
}

// ---------------------------------------------------------------------------
// Importação e capas
// ---------------------------------------------------------------------------

#[test]
fn import_valid_cbz_generates_real_cover_and_title() {
    let e = env();
    let p = make_cbz(&e.input("Minha_Primeira_HQ.cbz"), &as_files(&pages(3)));
    let id = e.import_ok(&p);
    let d = library::details(&e.conn, id).unwrap();
    assert_eq!(d.summary.title, "Minha Primeira HQ");
    assert_eq!(d.field_sources["title"], "filename");
    assert_eq!(d.summary.page_count, Some(3));
    assert_eq!(d.summary.cover_kind, "first_page");
    assert_eq!(d.summary.status, "unread");
    let info = library::file_info(&e.conn, id).unwrap();
    let cover = std::fs::read(e.covers.join(info.cover_file.unwrap())).unwrap();
    let img = image::load_from_memory(&cover).unwrap();
    assert_eq!(img.width(), 60, "imagens pequenas não são ampliadas");
}

#[test]
fn cover_preference_order() {
    let e = env();
    // 1. FrontCover do ComicInfo
    let xml = r#"<ComicInfo><Pages><Page Image="1" Type="FrontCover"/></Pages></ComicInfo>"#;
    let mut files = as_files(&pages(3));
    files.push(("ComicInfo.xml", xml.as_bytes().to_vec()));
    let id = e.import_ok(&make_cbz(&e.input("meta.cbz"), &files));
    let d = library::details(&e.conn, id).unwrap();
    assert_eq!((d.summary.cover_kind.as_str(), d.cover_page), ("metadata", Some(1)));

    // 2. Imagem nomeada como capa
    let mut files = as_files(&pages(3));
    files.push(("cover.jpg", jpeg(40, 60, 9)));
    let id = e.import_ok(&make_cbz(&e.input("nomeada.cbz"), &files));
    assert_eq!(library::details(&e.conn, id).unwrap().summary.cover_kind, "format");

    // 3. Primeira página inválida -> usa a próxima válida
    let files = vec![("1.png", b"nao e png".to_vec()), ("2.png", png(30, 40, 3))];
    let id = e.import_ok(&make_cbz(&e.input("primeira-ruim.cbz"), &files));
    let d = library::details(&e.conn, id).unwrap();
    assert_eq!((d.summary.cover_kind.as_str(), d.cover_page), ("first_page", Some(1)));
}

#[test]
fn placeholder_cover_when_no_page_decodes() {
    let e = env();
    let files = vec![("1.png", b"lixo".to_vec()), ("2.jpg", b"mais lixo".to_vec())];
    let r = e.import(&make_cbz(&e.input("sem-capa.cbz"), &files));
    assert_eq!(r.status, "added");
    assert!(r.message.unwrap().contains("capa substituta"));
    let d = library::details(&e.conn, r.comic_id.unwrap()).unwrap();
    assert_eq!(d.summary.cover_kind, "placeholder");
    assert!(d.summary.cover_error.is_some(), "o motivo da capa substituta é registrado");
}

#[test]
fn import_folder_of_images_and_nested_scan() {
    let e = env();
    let root = e.input("colecao");
    make_folder(&root.join("Volume 1"), &as_files(&pages(4)));
    make_folder(&root.join("Volume 2"), &as_files(&pages(2)));
    make_cbz(&root.join("extra.cbz"), &as_files(&pages(2)));
    std::fs::create_dir_all(root.join("vazia")).unwrap();
    let (cands, errors) = import::expand(&[root.clone()]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(cands.len(), 3);

    let cancel = AtomicBool::new(false);
    let job = library::create_import_job(&e.conn).unwrap();
    let (counts, items, cancelled) =
        import::run_import(&e.conn, &e.covers, job, &[root], &cancel, &mut |_, _, _, _| {});
    assert!(!cancelled);
    assert_eq!(counts.added, 3, "{items:?}");
    let folder = items.iter().find(|i| i.name == "Volume 1").unwrap();
    let d = library::details(&e.conn, folder.comic_id.unwrap()).unwrap();
    assert_eq!(d.summary.format, "folder");
    assert_eq!(d.summary.page_count, Some(4));
    assert_eq!(d.summary.cover_kind, "first_page");

    let empty = e.input("so-texto");
    make_folder(&empty, &[("a.txt", b"x".to_vec())]);
    let (_, errors) = import::expand(&[empty]);
    assert!(errors[0].message.as_ref().unwrap().contains("não contém imagens"));

    // Formatos de leitura não suportados dentro de pastas são relatados, não ignorados.
    let mixed = e.input("mista");
    make_folder(&mixed, &[("livro.epub", b"PK".to_vec()), ("leia.txt", b"x".to_vec())]);
    make_cbz(&mixed.join("ok.cbz"), &as_files(&pages(1)));
    let (cands, errors) = import::expand(&[mixed]);
    assert_eq!(cands.len(), 1);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].message.as_ref().unwrap().contains(".epub"));
}

#[test]
fn duplicates_by_content_not_by_name() {
    let e = env();
    let a = make_cbz(&e.input("original.cbz"), &as_files(&pages(3)));
    let id = e.import_ok(&a);
    // Mesmo conteúdo, nome diferente: duplicata.
    let copy = e.input("copia-com-outro-nome.cbz");
    std::fs::copy(&a, &copy).unwrap();
    let r = e.import(&copy);
    assert_eq!(r.status, "duplicate");
    assert_eq!(r.comic_id, Some(id));
    // Mesmo nome, conteúdo diferente: não é duplicata.
    std::fs::create_dir_all(e.input("outra-pasta")).unwrap();
    let other = make_cbz(&e.input("outra-pasta/original.cbz"), &as_files(&pages(5)));
    assert_eq!(e.import(&other).status, "added");
    // Mesmo caminho de novo: ignorado.
    assert_eq!(e.import(&a).status, "skipped");
    // Política "importar mesmo assim".
    let r = import::process_one(&e.conn, &e.covers, &copy, DuplicatePolicy::Allow);
    assert_eq!(r.status, "added");
}

#[test]
fn moved_file_is_relinked_and_keeps_progress() {
    let e = env();
    let a = make_cbz(&e.input("vai-mudar.cbz"), &as_files(&pages(10)));
    let id = e.import_ok(&a);
    library::mark_opened(&e.conn, id, 10).unwrap();
    library::save_progress(&e.conn, id, 6, 10, 0.0).unwrap();
    std::fs::create_dir_all(e.input("novo")).unwrap();
    let moved = e.input("novo/mudou.cbz");
    std::fs::rename(&a, &moved).unwrap();
    library::set_missing(&e.conn, id, true).unwrap();
    let r = e.import(&moved);
    assert_eq!(r.status, "relinked", "{r:?}");
    assert_eq!(r.comic_id, Some(id));
    let d = library::details(&e.conn, id).unwrap();
    assert!(!d.summary.missing);
    assert!(d.path.ends_with("mudou.cbz"));
    assert_eq!(d.summary.progress.unwrap().page_index, 6);
}

#[test]
fn all_formats_import_end_to_end() {
    let e = env();
    let pg = as_files(&pages(3));
    let ids = [
        e.import_ok(&make_cbz(&e.input("f.cbz"), &pg)),
        e.import_ok(&make_rar5(&e.input("f.cbr"), &pg)),
        e.import_ok(&make_cb7(&e.input("f.cb7"), &pg)),
        e.import_ok(&make_cbt(&e.input("f.cbt"), &pg)),
        e.import_ok(&make_folder(&e.input("f-pasta"), &pg)),
    ];
    for id in ids {
        let d = library::details(&e.conn, id).unwrap();
        assert_eq!(d.summary.page_count, Some(3), "{}", d.summary.format);
        assert_eq!(d.summary.cover_kind, "first_page", "{}", d.summary.format);
    }
    let pdf = e.import_ok(&make_pdf(&e.input("f.pdf"), 4, Some("Titulo do PDF")));
    let d = library::details(&e.conn, pdf).unwrap();
    assert_eq!(d.summary.title, "Titulo do PDF");
    assert_eq!(d.summary.page_count, Some(4));
    assert_eq!(d.summary.cover_kind, "pending", "capa do PDF é renderizada pela interface");
    assert_eq!(library::pending_covers(&e.conn, true).unwrap(), vec![pdf]);
}

// ---------------------------------------------------------------------------
// Serviço de páginas
// ---------------------------------------------------------------------------

#[test]
fn page_service_serves_every_format() {
    let e = env();
    let pg = pages(6);
    let files = as_files(&pg);
    let svc = PageService::new(e.root.join("pages-cache"));
    for path in [
        make_cbz(&e.input("s.cbz"), &files),
        make_rar5(&e.input("s.cbr"), &files),
        make_cb7(&e.input("s.cb7"), &files),
        make_cbt(&e.input("s.cbt"), &files),
        make_folder(&e.input("s-pasta"), &files),
    ] {
        let id = e.import_ok(&path);
        let info = library::file_info(&e.conn, id).unwrap();
        for i in [5usize, 0, 3] {
            let (bytes, mime) = svc.read_page(&info, i).unwrap();
            assert_eq!(bytes, pg[i].1, "{} página {i}", info.format);
            assert_eq!(mime, "image/png");
        }
        assert!(svc.read_page(&info, 6).is_err(), "página inexistente");
    }
    // RAR/7z ficam no cache de páginas; limpar não afeta os originais.
    assert!(svc.cache_size() > 0);
    svc.clear_cache();
    assert_eq!(svc.cache_size(), 0);
    assert!(e.input("s.cbr").exists());
}

#[test]
fn page_service_reports_missing_file() {
    let e = env();
    let p = make_cbz(&e.input("some.cbz"), &as_files(&pages(2)));
    let id = e.import_ok(&p);
    let info = library::file_info(&e.conn, id).unwrap();
    std::fs::remove_file(&p).unwrap();
    let svc = PageService::new(e.root.join("pc"));
    let err = svc.read_page(&info, 0).unwrap_err();
    assert_eq!(err.code(), "file_missing");
}

// ---------------------------------------------------------------------------
// Catálogo
// ---------------------------------------------------------------------------

fn seed(e: &Env) -> (i64, i64, i64) {
    let a = e.import_ok(&make_cbz(&e.input("Ação_Total_01.cbz"), &as_files(&pages(3))));
    let b = e.import_ok(&make_cb7(&e.input("Mangá Legal 2.cb7"), &as_files(&pages(4))));
    let c = e.import_ok(&make_folder(&e.input("Diário de Bordo"), &as_files(&pages(2))));
    (a, b, c)
}

fn titles(e: &Env, q: LibraryQuery) -> Vec<String> {
    library::list_comics(&e.conn, &q).unwrap().into_iter().map(|c| c.title).collect()
}

#[test]
fn search_filters_and_sorting() {
    let e = env();
    let (a, b, c) = seed(&e);
    // Busca sem acento e por nome de arquivo.
    assert_eq!(titles(&e, LibraryQuery { text: Some("acao".into()), ..Default::default() }), vec!["Ação Total 01"]);
    assert_eq!(titles(&e, LibraryQuery { text: Some("legal 2.cb7".into()), ..Default::default() }).len(), 1);
    // Tags e autor
    let mut conn = db::open(&e.db_path).unwrap();
    library::update_metadata(
        &mut conn,
        b,
        &MetadataPatch {
            title: "Mangá Legal 2".into(),
            series: Some("Legal".into()),
            issue_number: Some("2".into()),
            volume: None,
            year: Some(2020),
            writer: Some("Kenji Autor".into()),
            artist: None,
            publisher: None,
            summary: None,
            tags: vec!["shonen".into(), "comédia".into()],
        },
    )
    .unwrap();
    assert_eq!(titles(&e, LibraryQuery { text: Some("comedia".into()), ..Default::default() }), vec!["Mangá Legal 2"]);
    assert_eq!(titles(&e, LibraryQuery { text: Some("kenji".into()), ..Default::default() }), vec!["Mangá Legal 2"]);
    // Filtros
    library::set_favorite(&e.conn, c, true).unwrap();
    assert_eq!(titles(&e, LibraryQuery { favorites_only: true, ..Default::default() }), vec!["Diário de Bordo"]);
    assert_eq!(titles(&e, LibraryQuery { formats: vec!["cb7".into()], ..Default::default() }), vec!["Mangá Legal 2"]);
    library::mark_opened(&e.conn, a, 3).unwrap();
    assert_eq!(titles(&e, LibraryQuery { statuses: vec!["reading".into()], ..Default::default() }), vec!["Ação Total 01"]);
    assert_eq!(titles(&e, LibraryQuery { statuses: vec!["unread".into()], ..Default::default() }).len(), 2);
    // Ordenação por título (natural) e por progresso
    assert_eq!(
        titles(&e, LibraryQuery { sort: Some("title".into()), ..Default::default() }),
        vec!["Ação Total 01", "Diário de Bordo", "Mangá Legal 2"]
    );
    library::save_progress(&e.conn, a, 2, 3, 0.0).unwrap(); // concluída
    let by_progress = titles(&e, LibraryQuery { sort: Some("progress".into()), descending: true, ..Default::default() });
    assert_eq!(by_progress[0], "Ação Total 01");
    // Recentes: apenas abertas
    assert_eq!(titles(&e, LibraryQuery { opened_only: true, ..Default::default() }), vec!["Ação Total 01"]);
}

#[test]
fn edit_metadata_tracks_manual_fields() {
    let mut e = env();
    let id = e.import_ok(&make_cbz(&e.input("x.cbz"), &as_files(&pages(1))));
    let patch = MetadataPatch {
        title: "  Novo Título  ".into(),
        series: Some("Série".into()),
        issue_number: Some("5".into()),
        volume: None,
        year: Some(1999),
        writer: None,
        artist: Some("Arte".into()),
        publisher: None,
        summary: Some("Resumo".into()),
        tags: vec!["a".into(), "b".into()],
    };
    library::update_metadata(&mut e.conn, id, &patch).unwrap();
    let d = library::details(&e.conn, id).unwrap();
    assert_eq!(d.summary.title, "Novo Título");
    assert_eq!(d.field_sources["title"], "user");
    assert_eq!(d.field_sources["series"], "user");
    assert!(d.field_sources.get("writer").is_none(), "campo não alterado não muda de origem");
    assert_eq!(d.tags, vec!["a", "b"]);
    let bad = MetadataPatch { title: "   ".into(), ..patch.clone() };
    assert!(library::update_metadata(&mut e.conn, id, &bad).is_err());
    let bad_year = MetadataPatch { year: Some(12), ..patch };
    assert!(library::update_metadata(&mut e.conn, id, &bad_year).is_err());
}

#[test]
fn favorites_toggle() {
    let e = env();
    let id = e.import_ok(&make_cbz(&e.input("f.cbz"), &as_files(&pages(1))));
    library::set_favorite(&e.conn, id, true).unwrap();
    assert!(library::summary(&e.conn, id).unwrap().favorite);
    library::set_favorite(&e.conn, id, false).unwrap();
    assert!(!library::summary(&e.conn, id).unwrap().favorite);
    assert!(library::set_favorite(&e.conn, 9999, true).is_err());
}

#[test]
fn collections_lifecycle() {
    let mut e = env();
    let (a, b, _) = seed(&e);
    let col = library::create_collection(&e.conn, "Favoritas de 2024").unwrap();
    assert!(library::create_collection(&e.conn, "favoritas de 2024").is_err(), "nome repetido (sem diferenciar maiúsculas)");
    assert!(library::create_collection(&e.conn, "   ").is_err());
    library::add_to_collection(&mut e.conn, col, &[a, b]).unwrap();
    let list = library::list_collections(&e.conn).unwrap();
    assert_eq!(list[0].comic_count, 2);
    assert_eq!(list[0].preview.len(), 2);
    library::rename_collection(&e.conn, col, "Clássicos").unwrap();
    assert_eq!(library::list_collections(&e.conn).unwrap()[0].name, "Clássicos");
    // Busca dentro da coleção
    let found = library::list_comics(
        &e.conn,
        &LibraryQuery { collection_id: Some(col), text: Some("manga".into()), ..Default::default() },
    )
    .unwrap();
    assert_eq!(found.len(), 1);
    library::set_comic_collections(&mut e.conn, a, &[]).unwrap();
    assert_eq!(library::list_collections(&e.conn).unwrap()[0].comic_count, 1);
    // Excluir a coleção não remove as HQs
    library::delete_collection(&e.conn, col).unwrap();
    assert!(library::list_collections(&e.conn).unwrap().is_empty());
    assert_eq!(library::list_comics(&e.conn, &LibraryQuery::default()).unwrap().len(), 3);
}

#[test]
fn removing_from_library_keeps_original_file() {
    let e = env();
    let p = make_cbz(&e.input("fica.cbz"), &as_files(&pages(2)));
    let id = e.import_ok(&p);
    library::save_progress(&e.conn, id, 1, 2, 0.0).unwrap();
    library::delete_comic(&e.conn, id).unwrap();
    assert!(p.exists(), "o arquivo original continua no disco");
    assert!(library::summary(&e.conn, id).is_err());
    let orphan: i64 =
        e.conn.query_row("SELECT count(*) FROM reading_progress WHERE comic_id = ?", [id], |r| r.get(0)).unwrap();
    assert_eq!(orphan, 0, "progresso removido em cascata");
}

// ---------------------------------------------------------------------------
// Progresso
// ---------------------------------------------------------------------------

#[test]
fn progress_survives_reopening_the_database() {
    let e = env();
    let id = e.import_ok(&make_cbz(&e.input("longa.cbz"), &as_files(&pages(20))));
    let start = library::mark_opened(&e.conn, id, 20).unwrap();
    assert_eq!(start.page_index, 0);
    assert_eq!(library::summary(&e.conn, id).unwrap().status, "reading");
    library::save_progress(&e.conn, id, 11, 20, 0.42).unwrap();
    let db_path = e.db_path.clone();
    drop(e.conn); // "fecha o aplicativo"
    let conn = db::open(&db_path).unwrap();
    let s = library::summary(&conn, id).unwrap();
    let p = s.progress.unwrap();
    assert_eq!((p.page_index, p.page_count), (11, 20));
    assert!((p.scroll_ratio - 0.42).abs() < 1e-9);
    let resumed = library::mark_opened(&conn, id, 20).unwrap();
    assert_eq!(resumed.page_index, 11, "retoma na página salva");
}

#[test]
fn completion_reset_and_unread() {
    let e = env();
    let id = e.import_ok(&make_cbz(&e.input("c.cbz"), &as_files(&pages(5))));
    library::mark_opened(&e.conn, id, 5).unwrap();
    assert_eq!(library::save_progress(&e.conn, id, 4, 5, 0.0).unwrap(), "completed");
    // HQ concluída recomeça do início ao reabrir, mantendo o estado.
    assert_eq!(library::mark_opened(&e.conn, id, 5).unwrap().page_index, 0);
    assert_eq!(library::summary(&e.conn, id).unwrap().status, "completed");
    library::reset_progress(&e.conn, id).unwrap();
    let s = library::summary(&e.conn, id).unwrap();
    assert_eq!((s.status.as_str(), s.progress.unwrap().page_index), ("reading", 0));
    library::set_read_status(&e.conn, id, "unread").unwrap();
    let s = library::summary(&e.conn, id).unwrap();
    assert_eq!(s.status, "unread");
    assert!(s.progress.is_none());
    library::set_read_status(&e.conn, id, "completed").unwrap();
    assert_eq!(library::summary(&e.conn, id).unwrap().progress.unwrap().page_index, 4);
    // Valores fora do intervalo são limitados.
    library::save_progress(&e.conn, id, 99, 5, 7.0).unwrap();
    let p = library::summary(&e.conn, id).unwrap().progress.unwrap();
    assert_eq!(p.page_index, 4);
    assert!(p.scroll_ratio <= 1.0);
    assert!(library::save_progress(&e.conn, id, 0, 0, 0.0).is_err());
}

#[test]
fn clearing_cover_cache_keeps_progress_and_regenerates() {
    let e = env();
    let id = e.import_ok(&make_cbz(&e.input("k.cbz"), &as_files(&pages(4))));
    library::mark_opened(&e.conn, id, 4).unwrap();
    library::save_progress(&e.conn, id, 2, 4, 0.0).unwrap();
    for f in std::fs::read_dir(&e.covers).unwrap() {
        std::fs::remove_file(f.unwrap().path()).unwrap();
    }
    library::reset_all_covers(&e.conn).unwrap();
    assert_eq!(library::summary(&e.conn, id).unwrap().cover_kind, "pending");
    let emit: import::Emit = std::sync::Arc::new(|_, _| {});
    assert_eq!(import::regenerate_covers(&e.conn, &e.covers, &emit).unwrap(), 1);
    let s = library::summary(&e.conn, id).unwrap();
    assert_eq!(s.cover_kind, "first_page");
    assert_eq!(s.progress.unwrap().page_index, 2, "progresso preservado");
    let params: i64 = e.conn.query_row("SELECT cover_params FROM comics WHERE id = ?", [id], |r| r.get(0)).unwrap();
    assert_eq!(params, COVER_PARAMS_VERSION);
}

#[test]
fn settings_round_trip_and_validation() {
    let e = env();
    library::set_setting(&e.conn, "appearance.theme", &serde_json::json!("light")).unwrap();
    library::set_setting(&e.conn, "appearance.theme", &serde_json::json!("dark")).unwrap();
    let s = library::get_settings(&e.conn).unwrap();
    assert_eq!(s["appearance.theme"], "dark");
    assert!(library::set_setting(&e.conn, "chave inválida!", &serde_json::json!(1)).is_err());
    assert_eq!(import::duplicate_policy(&e.conn), DuplicatePolicy::Skip);
    library::set_setting(&e.conn, "library.duplicates", &serde_json::json!("allow")).unwrap();
    assert_eq!(import::duplicate_policy(&e.conn), DuplicatePolicy::Allow);
}

#[test]
fn import_job_cancellation() {
    let e = env();
    let paths: Vec<PathBuf> =
        (0..3).map(|i| make_cbz(&e.input(&format!("c{i}.cbz")), &as_files(&pages(1 + i)))).collect();
    let cancel = AtomicBool::new(true);
    let job = library::create_import_job(&e.conn).unwrap();
    let (counts, _, cancelled) = import::run_import(&e.conn, &e.covers, job, &paths, &cancel, &mut |_, _, _, _| {});
    assert!(cancelled);
    assert_eq!(counts.added, 0);
    let status: String = e.conn.query_row("SELECT status FROM import_jobs WHERE id = ?", [job], |r| r.get(0)).unwrap();
    assert_eq!(status, "cancelled");
}
