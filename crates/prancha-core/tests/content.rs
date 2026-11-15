//! Quadrinhos guardados como endereços `content://` (o seletor de pastas do Android). Um provedor de teste faz o
//! papel do sistema: os endereços são opacos, sem o nome do arquivo, como os da pasta Downloads do Android.

mod common;

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};

use common::*;
use prancha_core::core::Core;
use prancha_core::import::{self, DuplicatePolicy};
use prancha_core::library;
use prancha_core::location::{self, Entry, Meta, Provider};
use prancha_core::pages::PageService;

const PREFIX: &str = "content://prancha.test/document/";

/// Os arquivos de todos os testes ficam numa pasta só: o provedor é registrado uma vez por processo.
fn storage() -> &'static Path {
    static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
    ROOT.get_or_init(|| {
        location::set_provider(Arc::new(TestProvider));
        tempfile::tempdir().unwrap()
    })
    .path()
}

struct TestProvider;

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect();
    String::from_utf8(bytes).unwrap()
}

/// O endereço de um arquivo do armazenamento de teste.
fn uri(real: &Path) -> PathBuf {
    let rel = real.strip_prefix(storage()).unwrap().to_string_lossy().replace('\\', "/");
    PathBuf::from(format!("{PREFIX}{}", hex(&rel)))
}

fn real(uri: &str) -> io::Result<PathBuf> {
    let id = uri.strip_prefix(PREFIX).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "outro provedor"))?;
    Ok(storage().join(unhex(id)))
}

impl Provider for TestProvider {
    fn open(&self, uri: &str) -> io::Result<File> {
        File::open(real(uri)?)
    }

    fn stat(&self, uri: &str) -> io::Result<(String, Meta)> {
        let path = real(uri)?;
        let meta = std::fs::metadata(&path)?;
        Ok((path.file_name().unwrap().to_string_lossy().to_string(), meta.into()))
    }

    fn children(&self, uri: &str) -> io::Result<Vec<Entry>> {
        let dir = real(uri)?;
        let mut out = Vec::new();
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let meta = e.metadata()?;
            out.push(Entry {
                path: self::uri(&e.path()),
                name: e.file_name().to_string_lossy().to_string(),
                is_file: meta.is_file(),
                len: if meta.is_file() { meta.len() } else { 0 },
            });
        }
        Ok(out)
    }
}

/// Uma pasta nova dentro do armazenamento de teste.
fn folder(name: &str) -> PathBuf {
    let dir = storage().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn core(name: &str) -> Core {
    let data = tempfile::tempdir().unwrap().keep().join(name);
    Core::init(data, Arc::new(|_, _| {})).unwrap()
}

#[test]
fn every_format_imports_and_reads_through_content_addresses() {
    let dir = folder("formatos");
    let core = core("formatos");
    let pg = pages(4);
    let files = as_files(&pg);
    let svc = PageService::new(dir.join("cache-paginas"));
    let mut comics = vec![
        ("cbz", make_cbz(&dir.join("Volume_01.cbz"), &files), "Volume 01"),
        ("cb7", make_cb7(&dir.join("Volume_03.cb7"), &files), "Volume 03"),
        ("cbt", make_cbt(&dir.join("Volume_04.cbt"), &files), "Volume 04"),
        ("folder", make_folder(&dir.join("Volume 05"), &files), "Volume 05"),
    ];
    // O UnRAR abre o descritor por /proc/self/fd, que só existe no Linux e no Android (no Windows não há content://).
    if cfg!(unix) {
        comics.push(("cbr", make_rar5(&dir.join("Volume_02.cbr"), &files), "Volume 02"));
    }
    for (format, real, title) in comics {
        let address = uri(&real);
        assert!(!address.to_string_lossy().contains("Volume"), "o endereço de teste não pode ter o nome");
        let r = import::process_one(&core.db(), &core.paths.covers, &address, DuplicatePolicy::Skip);
        assert_eq!(r.status, "added", "{format}: {r:?}");
        let id = r.comic_id.unwrap();
        let d = library::details(&core.db(), id).unwrap();
        assert_eq!(d.summary.format, format);
        assert_eq!(d.summary.title, title, "o título vem do nome que o sistema informa");
        assert_eq!(d.summary.page_count, Some(4), "{format}");
        assert_eq!(d.summary.cover_kind, "first_page", "{format}");
        assert_eq!(Path::new(&d.path), address, "a biblioteca guarda o endereço, não um caminho");
        let info = library::file_info(&core.db(), id).unwrap();
        for i in [3usize, 0, 2] {
            let (bytes, mime) = svc.read_page(&info, i).unwrap();
            assert_eq!(bytes, pg[i].1, "{format} página {i}");
            assert_eq!(mime, "image/png");
        }
    }

    let pdf = uri(&make_pdf(&dir.join("livro.pdf"), 3, Some("Titulo do PDF")));
    let r = import::process_one(&core.db(), &core.paths.covers, &pdf, DuplicatePolicy::Skip);
    assert_eq!(r.status, "added", "{r:?}");
    let d = library::details(&core.db(), r.comic_id.unwrap()).unwrap();
    assert_eq!((d.summary.format.as_str(), d.summary.title.as_str(), d.summary.page_count), ("pdf", "Titulo do PDF", Some(3)));

    // A mesma HQ de novo, por outro endereço com o mesmo conteúdo, é reconhecida como repetida.
    let copy = dir.join("copia.cbz");
    std::fs::copy(dir.join("Volume_01.cbz"), &copy).unwrap();
    let r = import::process_one(&core.db(), &core.paths.covers, &uri(&copy), DuplicatePolicy::Skip);
    assert_eq!(r.status, "duplicate", "{r:?}");
}

#[test]
fn a_picked_folder_is_scanned_like_a_folder_on_disk() {
    let root = folder("pasta-escolhida");
    let core = core("pasta-escolhida");
    let files = as_files(&pages(2));
    make_cbz(&root.join("solta.cbz"), &files);
    std::fs::create_dir_all(root.join("serie")).unwrap();
    make_cb7(&root.join("serie").join("cap1.cb7"), &files);
    make_folder(&root.join("serie").join("cap2"), &files);
    std::fs::write(root.join("leia-me.txt"), b"nada").unwrap();

    let (found, errors) = import::expand(&[uri(&root)]);
    assert!(errors.is_empty(), "{errors:?}");
    let mut names: Vec<String> = found.iter().map(|p| location::file_name(p).unwrap()).collect();
    names.sort();
    assert_eq!(names, vec!["cap1.cb7", "cap2", "solta.cbz"]);

    let job = library::create_import_job(&core.db()).unwrap();
    let cancel = AtomicBool::new(false);
    let (counts, items, cancelled) = import::run_import(&core.db(), &core.paths.covers, job, &[uri(&root)], &cancel, &mut |_, _, _, _| {});
    assert!(!cancelled);
    assert_eq!(counts.added, 3, "{items:?}");
}

#[test]
fn a_removed_document_is_marked_missing_and_comes_back() {
    let dir = folder("verificacao");
    let core = core("verificacao");
    let real = make_cbz(&dir.join("some.cbz"), &as_files(&pages(2)));
    let r = import::process_one(&core.db(), &core.paths.covers, &uri(&real), DuplicatePolicy::Skip);
    let id = r.comic_id.unwrap();

    let kept = dir.join("guardado");
    std::fs::rename(&real, &kept).unwrap();
    assert!(!location::exists(&uri(&real)));
    assert_eq!(core.verify_files().unwrap(), (1, 0));
    assert!(library::details(&core.db(), id).unwrap().summary.missing);
    let info = library::file_info(&core.db(), id).unwrap();
    assert_eq!(core.pages.read_page(&info, 0).unwrap_err().code(), "file_missing");

    std::fs::rename(&kept, &real).unwrap();
    assert_eq!(core.verify_files().unwrap(), (0, 1));
    assert!(!library::details(&core.db(), id).unwrap().summary.missing);
}
