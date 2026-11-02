//! `prancha-ext`: gera chaves, monta componentes e publica repositórios de extensões.
//! Ver `docs/extensions/PUBLISHING.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const USAGE: &str = "\
Uso:
  prancha-ext keygen <chave-privada>
      Cria um par de chaves. A chave privada nunca deve ir para um repositório.
  prancha-ext component <modulo.wasm> <componente.wasm>
      Transforma o módulo compilado em um componente.
  prancha-ext publish --key <chave-privada> --repo <repositorio.json> --out <pasta> <extensao.json>...
      Gera prancha-repo.json, prancha-index.json e as assinaturas.
  prancha-ext verify <pasta>
      Confere as assinaturas e os SHA-256 de um repositório gerado.";

type Res<T> = Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["keygen", path] => keygen(Path::new(path)),
        ["component", input, output] => component(Path::new(input), Path::new(output)),
        ["publish", rest @ ..] => publish(rest),
        ["verify", dir] => verify(Path::new(dir)),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn read(path: &Path) -> Res<Vec<u8>> {
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) -> Res<()> {
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_json(path: &Path) -> Res<Value> {
    serde_json::from_slice(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

// As mensagens assinadas têm prefixo próprio: a assinatura de um índice nunca vale como a de um pacote.
// O app monta as mesmas mensagens em `src-tauri/src/extensions/signing.rs`.

fn index_message(index: &[u8]) -> Vec<u8> {
    [b"prancha-index-v1\0".as_slice(), index].concat()
}

fn package_message(repository: &str, extension: &str, version: &str, sha256: &str) -> Vec<u8> {
    format!("prancha-package-v1\0{repository}\0{extension}\0{version}\0{sha256}").into_bytes()
}

fn fingerprint(public_key: &[u8]) -> String {
    sha256(public_key)
}

fn load_key(path: &Path) -> Res<SigningKey> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = B64.decode(text.trim()).map_err(|e| format!("Chave privada inválida: {e}"))?;
    let seed: [u8; 32] = bytes.try_into().map_err(|_| "Chave privada inválida: tamanho errado.".to_string())?;
    Ok(SigningKey::from_bytes(&seed))
}

fn keygen(path: &Path) -> Res<()> {
    if path.exists() {
        return Err(format!("{} já existe e não foi alterado.", path.display()));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
    write(path, B64.encode(seed))?;
    let key = SigningKey::from_bytes(&seed);
    let public = key.verifying_key();
    println!("Chave pública: {}", B64.encode(public.as_bytes()));
    println!("Impressão digital (SHA-256): {}", fingerprint(public.as_bytes()));
    Ok(())
}

fn component(input: &Path, output: &Path) -> Res<()> {
    let module = read(input)?;
    let bytes = wit_component::ComponentEncoder::default()
        .module(&module)
        .and_then(|e| e.validate(true).encode())
        .map_err(|e| format!("{e:#}"))?;
    write(output, bytes)
}

struct PublishArgs {
    key: PathBuf,
    repo: PathBuf,
    out: PathBuf,
    extensions: Vec<PathBuf>,
}

fn publish_args(args: &[&str]) -> Res<PublishArgs> {
    let (mut key, mut repo, mut out, mut extensions) = (None, None, None, Vec::new());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().map(PathBuf::from).ok_or_else(|| USAGE.to_string());
        match *arg {
            "--key" => key = Some(value()?),
            "--repo" => repo = Some(value()?),
            "--out" => out = Some(value()?),
            other => extensions.push(PathBuf::from(other)),
        }
    }
    match (key, repo, out) {
        (Some(key), Some(repo), Some(out)) => Ok(PublishArgs { key, repo, out, extensions }),
        _ => Err(USAGE.to_string()),
    }
}

/// Texto obrigatório de um JSON de entrada.
fn text<'a>(value: &'a Value, field: &str, file: &Path) -> Res<&'a str> {
    value[field].as_str().ok_or_else(|| format!("{}: falta o campo `{field}`.", file.display()))
}

fn now_rfc3339() -> Res<String> {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs() as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Conversão de dias para data civil (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    Ok(format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60))
}

fn publish(args: &[&str]) -> Res<()> {
    let args = publish_args(args)?;
    let key = load_key(&args.key)?;
    let repo = read_json(&args.repo)?;
    let repo_id = text(&repo, "id", &args.repo)?;
    let base = text(&repo, "baseUrl", &args.repo)?.trim_end_matches('/');
    fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;

    let mut entries = Vec::new();
    for file in &args.extensions {
        let mut entry = read_json(file)?;
        let dir = file.parent().unwrap_or(Path::new("."));
        let artifact = dir.join(text(&entry, "artifact", file)?);
        let id = text(&entry, "id", file)?.to_string();
        let version = text(&entry, "version", file)?.to_string();
        let bytes = read(&artifact)?;
        let hash = sha256(&bytes);
        let name = format!("{id}-{version}.wasm");
        write(&args.out.join(&name), &bytes)?;
        let signature = key.sign(&package_message(repo_id, &id, &version, &hash));
        write(&args.out.join(format!("{name}.sig")), B64.encode(signature.to_bytes()))?;
        let fields = entry.as_object_mut().ok_or_else(|| format!("{}: precisa ser um objeto.", file.display()))?;
        fields.remove("artifact");
        fields.insert("runtime".into(), json!("wasm-component"));
        fields.insert("apiVersion".into(), json!("1"));
        fields.insert("artifactUrl".into(), json!(format!("{base}/{name}")));
        fields.insert("signatureUrl".into(), json!(format!("{base}/{name}.sig")));
        fields.insert("size".into(), json!(bytes.len()));
        fields.insert("sha256".into(), json!(hash));
        entries.push(entry);
    }

    let now = now_rfc3339()?;
    let index = json!({ "schemaVersion": 1, "repositoryId": repo_id, "generatedAt": now, "extensions": entries });
    let index_bytes = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    write(&args.out.join("prancha-index.json"), &index_bytes)?;
    write(&args.out.join("prancha-index.json.sig"), B64.encode(key.sign(&index_message(&index_bytes)).to_bytes()))?;

    let public = key.verifying_key();
    let manifest = json!({
        "schemaVersion": 1,
        "id": repo_id,
        "name": text(&repo, "name", &args.repo)?,
        "description": repo["description"],
        "website": repo["website"],
        "indexUrl": format!("{base}/prancha-index.json"),
        "publicKey": B64.encode(public.as_bytes()),
        "updatedAt": now,
    });
    write(&args.out.join("prancha-repo.json"), serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)?;
    println!("Repositório em {}", args.out.display());
    println!("Impressão digital (SHA-256): {}", fingerprint(public.as_bytes()));
    Ok(())
}

fn signature_at(path: &Path) -> Res<Signature> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = B64.decode(text.trim()).map_err(|e| format!("{}: {e}", path.display()))?;
    Signature::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn file_of_url(url: &str) -> Res<&str> {
    url.rsplit('/').next().filter(|n| !n.is_empty()).ok_or_else(|| format!("Endereço sem nome de arquivo: {url}"))
}

fn verify(dir: &Path) -> Res<()> {
    let manifest_path = dir.join("prancha-repo.json");
    let manifest = read_json(&manifest_path)?;
    let public = B64.decode(text(&manifest, "publicKey", &manifest_path)?).map_err(|e| e.to_string())?;
    let public: [u8; 32] = public.as_slice().try_into().map_err(|_| "Chave pública com tamanho errado.".to_string())?;
    let key = VerifyingKey::from_bytes(&public).map_err(|e| e.to_string())?;
    println!("Impressão digital (SHA-256): {}", fingerprint(&public));

    let index_bytes = read(&dir.join("prancha-index.json"))?;
    let signature = signature_at(&dir.join("prancha-index.json.sig"))?;
    key.verify(&index_message(&index_bytes), &signature).map_err(|_| "A assinatura do índice não confere.".to_string())?;
    let index: Value = serde_json::from_slice(&index_bytes).map_err(|e| e.to_string())?;
    let repo_id = index["repositoryId"].as_str().ok_or("O índice não tem repositoryId.")?;
    println!("Índice: assinatura correta.");

    for entry in index["extensions"].as_array().ok_or("O índice não tem extensions.")? {
        let (id, version) = (entry["id"].as_str().unwrap_or_default(), entry["version"].as_str().unwrap_or_default());
        let bytes = read(&dir.join(file_of_url(entry["artifactUrl"].as_str().unwrap_or_default())?))?;
        let hash = sha256(&bytes);
        if entry["sha256"].as_str() != Some(hash.as_str()) {
            return Err(format!("{id} {version}: o SHA-256 não confere."));
        }
        let signature = signature_at(&dir.join(file_of_url(entry["signatureUrl"].as_str().unwrap_or_default())?))?;
        key.verify(&package_message(repo_id, id, version, &hash), &signature).map_err(|_| format!("{id} {version}: a assinatura não confere."))?;
        println!("{id} {version}: SHA-256 e assinatura corretos.");
    }
    Ok(())
}
