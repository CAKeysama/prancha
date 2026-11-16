// Gera o latest.json que o aplicativo lê para saber da versão nova. Vai junto dos instaladores em cada release.
// Uso: node scripts/updater-manifest.mjs <versão> <pasta com os instaladores e os .sig>
//      node scripts/updater-manifest.mjs <versão> <pasta> --android  (acrescenta o APK ao latest.json que já está na pasta)
import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { ROOT, readChangelog, releaseNotes } from "./changelog.mjs";

/** O endereço dos pacotes sai do mesmo endpoint que o aplicativo usa, para os dois nunca divergirem. */
export function packageBaseUrl(endpoint, version) {
  const suffix = "/latest/download/latest.json";
  if (!endpoint.endsWith(suffix)) throw new Error(`Endpoint inesperado: ${endpoint}`);
  return `${endpoint.slice(0, -suffix.length)}/download/v${version}`;
}

/** A assinatura em base64 precisa declarar a versão: o aplicativo recusa um pacote antigo apresentado como novo. */
export function assertSignedVersion(signature, version, name) {
  const text = Buffer.from(signature, "base64").toString("utf8");
  if (!text.split(/\s+/).some((part) => part === `version:${version}`)) {
    throw new Error(`A assinatura de ${name} não declara a versão ${version}.`);
  }
}

/** Lê os arquivos de uma pasta: devolve o conteúdo, ou `null` se o arquivo não existir. */
export function folderReader(dir) {
  return (name) => (existsSync(join(dir, name)) ? readFileSync(join(dir, name)) : null);
}

function describePackage(read, baseUrl, name, version) {
  const bytes = read(name);
  const sig = read(`${name}.sig`);
  if (!bytes) throw new Error(`Pacote não encontrado: ${name}`);
  if (!sig) throw new Error(`Assinatura não encontrada: ${name}.sig`);
  const signature = Buffer.from(sig).toString("utf8").trim();
  assertSignedVersion(signature, version, name);
  return { url: `${baseUrl}/${name}`, signature, sha256: createHash("sha256").update(bytes).digest("hex") };
}

/** `nsis` é o pacote principal: quem instalou pelo MSI continua no MSI, e os demais recebem o instalador padrão. */
export function buildManifest({ version, notes, date, read, baseUrl, nsis, msi }) {
  const main = describePackage(read, baseUrl, nsis, version);
  const platforms = { "windows-x86_64": main, "windows-x86_64-nsis": main };
  if (msi) platforms["windows-x86_64-msi"] = describePackage(read, baseUrl, msi, version);
  return { version, notes, pub_date: `${date}T12:00:00Z`, platforms };
}

/** O nome do APK publicado: só ARM de 64 bits, a arquitetura dos celulares atuais. */
export const apkName = (version) => `Prancha_${version}_android-arm64.apk`;

/**
 * O APK entra no latest.json depois dos instaladores, porque é gerado em outra máquina. Só o app Android lê a entrada
 * `android-aarch64` (crates/prancha-app/src/updates.rs); o plugin do desktop escolhe a sua pelo sistema e ignora esta.
 */
export function addAndroid(manifest, { read, baseUrl, apk }) {
  return { ...manifest, platforms: { ...manifest.platforms, "android-aarch64": describePackage(read, baseUrl, apk, manifest.version) } };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [version, dir, mode] = process.argv.slice(2);
  const entry = readChangelog().find((e) => e.version === version);
  if (!entry) throw new Error(`A versão ${version} não está no CHANGELOG.md.`);
  const config = JSON.parse(readFileSync(join(ROOT, "apps", "desktop", "src-tauri", "tauri.conf.json"), "utf8"));
  const baseUrl = packageBaseUrl(config.plugins.updater.endpoints[0], version);
  if (mode === "--android") {
    const current = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
    if (current.version !== version) throw new Error(`O latest.json da pasta é da versão ${current.version}, não da ${version}.`);
    const manifest = addAndroid(current, { read: folderReader(dir), baseUrl, apk: apkName(version) });
    writeFileSync(join(dir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
    process.exit(0);
  }
  const manifest = buildManifest({
    version,
    notes: releaseNotes(entry),
    date: entry.date,
    read: folderReader(dir),
    baseUrl,
    nsis: `Prancha_${version}_x64-setup.exe`,
    msi: `Prancha_${version}_x64_pt-BR.msi`,
  });
  writeFileSync(join(dir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
}
