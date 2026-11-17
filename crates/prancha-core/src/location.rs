//! Onde fica um quadrinho: um caminho comum ou, no Android, um endereço `content://` que o seletor de pastas
//! do sistema devolve. O resto do núcleo continua guardando e passando um `Path`; só as leituras do disco passam
//! por aqui. Sem um [`Provider`] registrado (o desktop), tudo é `std::fs`, exatamente como antes.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

/// O que se sabe de um arquivo ou de uma pasta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub is_dir: bool,
    pub is_file: bool,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl From<std::fs::Metadata> for Meta {
    fn from(m: std::fs::Metadata) -> Self {
        Meta { is_dir: m.is_dir(), is_file: m.is_file(), len: m.len(), modified: m.modified().ok() }
    }
}

/// Um item de uma pasta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    /// Arquivo comum: pastas (e, num caminho comum, atalhos) ficam `false`.
    pub is_file: bool,
    pub len: u64,
}

/// Quem lê os endereços `content://`. No Android é o plugin de arquivos (Kotlin), chamado de uma thread de trabalho.
pub trait Provider: Send + Sync {
    /// Abre para leitura. O descritor é do processo: fechar o `File` fecha o descritor.
    fn open(&self, uri: &str) -> io::Result<File>;
    /// Nome, tipo, tamanho e data. `NotFound` quando o documento não existe mais ou a permissão foi revogada.
    fn stat(&self, uri: &str) -> io::Result<(String, Meta)>;
    /// Itens de uma pasta, sem entrar nas subpastas.
    fn children(&self, uri: &str) -> io::Result<Vec<Entry>>;
}

static PROVIDER: OnceLock<Arc<dyn Provider>> = OnceLock::new();

/// Registra quem lê os endereços `content://`. Só a primeira chamada vale.
pub fn set_provider(provider: Arc<dyn Provider>) {
    let _ = PROVIDER.set(provider);
}

/// `true` para um endereço do seletor do Android.
pub fn is_content(path: &Path) -> bool {
    path.to_str().is_some_and(|s| s.starts_with("content://"))
}

fn provider() -> io::Result<&'static Arc<dyn Provider>> {
    PROVIDER.get().ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "este sistema não abre endereços content://"))
}

fn uri(path: &Path) -> &str {
    path.to_str().unwrap_or_default()
}

pub fn open(path: &Path) -> io::Result<File> {
    if is_content(path) { provider()?.open(uri(path)) } else { File::open(path) }
}

pub fn metadata(path: &Path) -> io::Result<Meta> {
    if is_content(path) { provider()?.stat(uri(path)).map(|(_, m)| m) } else { std::fs::metadata(path).map(Meta::from) }
}

pub fn exists(path: &Path) -> bool {
    if is_content(path) { provider().and_then(|p| p.stat(uri(path))).is_ok() } else { path.exists() }
}

/// O nome que a pessoa vê. Num endereço `content://` o nome não está no endereço: vem do sistema.
pub fn file_name(path: &Path) -> Option<String> {
    if is_content(path) {
        provider().and_then(|p| p.stat(uri(path))).ok().map(|(name, _)| name)
    } else {
        path.file_name().map(|n| n.to_string_lossy().to_string())
    }
}

/// Itens de uma pasta. Num caminho comum, só os que o sistema consegue descrever (como fazia o `read_dir`).
pub fn read_dir(path: &Path) -> io::Result<Vec<Entry>> {
    if is_content(path) {
        return provider()?.children(uri(path));
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        out.push(Entry {
            path: entry.path(),
            name: entry.file_name().to_string_lossy().to_string(),
            is_file: ft.is_file(),
            len: if ft.is_file() { entry.metadata().map(|m| m.len()).unwrap_or(0) } else { 0 },
        });
    }
    Ok(out)
}

/// Um arquivo dentro de uma pasta, pelo nome. Num endereço `content://` o filho não se monta juntando texto.
pub fn child(dir: &Path, name: &str) -> io::Result<PathBuf> {
    if !is_content(dir) {
        return Ok(dir.join(name));
    }
    provider()?
        .children(uri(dir))?
        .into_iter()
        .find(|e| e.name == name && e.is_file)
        .map(|e| e.path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{name} não está mais na pasta")))
}

/// Um caminho que uma biblioteca em C consegue abrir (o UnRAR só aceita nome de arquivo) e o arquivo aberto que o
/// mantém válido. Num endereço `content://` é `/proc/self/fd/<n>` do descritor que o sistema entregou.
pub struct NativePath {
    path: PathBuf,
    _file: Option<File>,
}

impl NativePath {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn native_path(path: &Path) -> io::Result<NativePath> {
    if !is_content(path) {
        return Ok(NativePath { path: path.to_path_buf(), _file: None });
    }
    let file = open(path)?;
    Ok(NativePath { path: fd_path(&file), _file: Some(file) })
}

#[cfg(unix)]
fn fd_path(file: &File) -> PathBuf {
    use std::os::fd::AsRawFd;
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

#[cfg(not(unix))]
fn fd_path(_file: &File) -> PathBuf {
    PathBuf::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_use_the_file_system() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.cbz");
        std::fs::write(&file, b"abc").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        assert!(!is_content(&file));
        assert!(exists(&file));
        assert_eq!(metadata(&file).unwrap().len, 3);
        assert!(metadata(dir.path()).unwrap().is_dir);
        assert_eq!(file_name(&file).as_deref(), Some("a.cbz"));
        assert_eq!(child(dir.path(), "a.cbz").unwrap(), file);
        assert_eq!(native_path(&file).unwrap().path(), file);
        let mut names: Vec<(String, bool, u64)> = read_dir(dir.path()).unwrap().into_iter().map(|e| (e.name, e.is_file, e.len)).collect();
        names.sort();
        assert_eq!(names, vec![("a.cbz".into(), true, 3), ("sub".into(), false, 0)]);
    }

    #[test]
    fn content_addresses_are_recognised() {
        assert!(is_content(Path::new("content://com.android.externalstorage.documents/document/primary%3ADownload%2Fa.cbz")));
        assert!(!is_content(Path::new("/storage/emulated/0/Download/a.cbz")));
        assert!(!is_content(Path::new(r"C:\HQs\content.cbz")));
    }
}
