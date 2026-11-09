//! Central de notificações: histórico persistente de eventos que pedem a atenção da pessoa.
//!
//! Quem cria uma notificação descreve o evento (`NewNotification`) com uma chave de deduplicação
//! por evento. `record` grava tudo numa transação: eventos já vistos são descartados, eventos do
//! mesmo grupo num intervalo curto somam na notificação ainda não lida, e as preferências decidem
//! se ela entra na central e se vira uma notificação do sistema. `publish` faz o mesmo e avisa a
//! interface (`notifications:changed`, com a contagem de não lidas) e o sistema operacional.
//!
//! Ler uma notificação nunca toca no capítulo, na obra ou no progresso: são registros separados.

use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::Core;
use crate::db::now_ms;
use crate::error::{AppError, AppResult};
use crate::library::{self, ADULT_ALLOWED_SQL, SERIES_SAFE_SQL};

pub const EVENT_CHANGED: &str = "notifications:changed";
/// Clique numa notificação do sistema: a interface executa a ação dela.
pub const EVENT_ACTIVATE: &str = "notifications:activate";

/// Itens guardados por notificação agrupada, para o texto citar os primeiros.
const MAX_ITEMS: usize = 20;
const MAX_TEXT_CHARS: usize = 200;
const MAX_PAGE: i64 = 100;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Política de retenção: lidas ficam 90 dias; qualquer uma, um ano; no máximo 2000.
const KEEP_READ_MS: i64 = 90 * DAY_MS;
const KEEP_ANY_MS: i64 = 365 * DAY_MS;
const KEEP_MAX: i64 = 2000;
/// Chaves de eventos duram mais que as notificações: um evento antigo não volta depois da limpeza.
const KEEP_EVENTS_MS: i64 = 400 * DAY_MS;

// ---------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Chapters,
    Updates,
    System,
    Downloads,
    Library,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Chapters => "chapters",
            Category::Updates => "updates",
            Category::System => "system",
            Category::Downloads => "downloads",
            Category::Library => "library",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "chapters" => Category::Chapters,
            "updates" => Category::Updates,
            "system" => Category::System,
            "downloads" => Category::Downloads,
            "library" => Category::Library,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    ChaptersNew,
    UpdateAvailable,
    UpdateInstalled,
    UpdateFailed,
    DownloadCompleted,
    DownloadFailed,
    SyncFailed,
    ImportProblems,
    FilesMissing,
}

/// O que uma notificação pode fazer. O destino vem das referências gravadas, nunca de uma rota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    OpenSeries,
    OpenChapter,
    OpenComic,
    OpenDownloads,
    OpenUpdates,
    OpenLibrary,
    RetrySync,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Action::OpenSeries => "open_series",
            Action::OpenChapter => "open_chapter",
            Action::OpenComic => "open_comic",
            Action::OpenDownloads => "open_downloads",
            Action::OpenUpdates => "open_updates",
            Action::OpenLibrary => "open_library",
            Action::RetrySync => "retry_sync",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        serde_json::from_value(Value::String(text.to_string())).ok()
    }
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::ChaptersNew => "chapters_new",
            Kind::UpdateAvailable => "update_available",
            Kind::UpdateInstalled => "update_installed",
            Kind::UpdateFailed => "update_failed",
            Kind::DownloadCompleted => "download_completed",
            Kind::DownloadFailed => "download_failed",
            Kind::SyncFailed => "sync_failed",
            Kind::ImportProblems => "import_problems",
            Kind::FilesMissing => "files_missing",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        serde_json::from_value(Value::String(text.to_string())).ok()
    }

    pub fn category(self) -> Category {
        match self {
            Kind::ChaptersNew => Category::Chapters,
            Kind::UpdateAvailable | Kind::UpdateInstalled | Kind::UpdateFailed => Category::Updates,
            Kind::DownloadCompleted | Kind::DownloadFailed => Category::Downloads,
            Kind::SyncFailed => Category::System,
            Kind::ImportProblems | Kind::FilesMissing => Category::Library,
        }
    }

    /// A configuração que decide se o evento entra na central.
    pub fn setting(self) -> &'static str {
        match self {
            Kind::ChaptersNew => "notifications.chapters",
            Kind::UpdateAvailable | Kind::UpdateInstalled | Kind::UpdateFailed => "notifications.updates",
            Kind::DownloadCompleted => "notifications.downloadsCompleted",
            Kind::DownloadFailed => "notifications.downloadsFailed",
            Kind::SyncFailed => "notifications.sync",
            Kind::ImportProblems | Kind::FilesMissing => "notifications.library",
        }
    }

    /// Só estes podem aparecer fora da janela. Downloads concluídos, só para quem pedir.
    fn native_setting(self) -> Option<(&'static str, bool)> {
        match self {
            Kind::ChaptersNew | Kind::UpdateAvailable | Kind::SyncFailed => Some(("notifications.native", true)),
            Kind::DownloadCompleted => Some(("notifications.nativeDownloads", false)),
            _ => None,
        }
    }

    /// Ações oferecidas, a primeira é a principal.
    pub fn actions(self, count: i64) -> &'static [Action] {
        match self {
            Kind::ChaptersNew if count == 1 => &[Action::OpenChapter, Action::OpenSeries],
            Kind::ChaptersNew => &[Action::OpenSeries, Action::OpenChapter],
            Kind::UpdateAvailable | Kind::UpdateFailed | Kind::UpdateInstalled => &[Action::OpenUpdates],
            Kind::DownloadCompleted => &[Action::OpenSeries, Action::OpenDownloads],
            Kind::DownloadFailed => &[Action::OpenDownloads, Action::OpenSeries],
            Kind::SyncFailed => &[Action::RetrySync],
            Kind::ImportProblems => &[Action::OpenLibrary],
            Kind::FilesMissing if count == 1 => &[Action::OpenComic],
            Kind::FilesMissing => &[Action::OpenLibrary],
        }
    }

    /// Título e texto a partir do que o evento juntou. Recalculado quando eventos se somam.
    fn compose(self, count: i64, subject: &str, items: &[String], extra: &Value) -> (String, String) {
        let version = extra.get("version").and_then(Value::as_str).unwrap_or("");
        let first = items.first().map(String::as_str).unwrap_or("");
        match self {
            Kind::ChaptersNew if count <= 1 => (format!("Capítulo novo em {subject}"), format!("{first} está disponível.")),
            Kind::ChaptersNew => (format!("{subject} recebeu {count} capítulos novos"), list_items(items, count)),
            Kind::UpdateAvailable => ("Nova versão disponível".into(), format!("O Prancha v{version} está pronto para instalar.")),
            Kind::UpdateInstalled => ("Atualização concluída".into(), format!("O Prancha foi atualizado para a v{version}.")),
            Kind::UpdateFailed => ("A atualização não foi concluída".into(), format!("A instalação da v{version} não terminou. A versão atual continua funcionando.")),
            Kind::DownloadCompleted if count <= 1 => ("Download concluído".into(), format!("{first}, de {subject}, está na biblioteca.")),
            Kind::DownloadCompleted => (format!("{count} capítulos baixados"), format!("Os capítulos de {subject} estão na biblioteca.")),
            Kind::DownloadFailed if count <= 1 => ("Falha no download".into(), format!("{first}, de {subject}, não foi baixado. Você pode tentar de novo em Downloads.")),
            Kind::DownloadFailed => (format!("{count} downloads falharam"), format!("Capítulos de {subject} não foram baixados. Você pode tentar de novo em Downloads.")),
            Kind::SyncFailed if count <= 1 => ("Não foi possível verificar novidades".into(), format!("A fonte de {first} não respondeu.")),
            Kind::SyncFailed => ("Não foi possível verificar novidades".into(), format!("{count} obras acompanhadas não puderam ser verificadas.")),
            Kind::ImportProblems => {
                let failed = extra.get("failed").and_then(Value::as_i64).unwrap_or(count);
                let total = extra.get("total").and_then(Value::as_i64).unwrap_or(failed);
                let noun = if failed == 1 { "arquivo não foi importado" } else { "arquivos não foram importados" };
                ("Importação com problemas".into(), format!("{failed} de {total} {noun}. Os detalhes estão no painel de importação."))
            }
            Kind::FilesMissing if count <= 1 => ("Arquivo não encontrado".into(), format!("O arquivo de {first} foi movido ou excluído.")),
            Kind::FilesMissing => ("Arquivos não encontrados".into(), format!("{count} HQs estão com o arquivo ausente. Os detalhes de cada uma oferecem Localizar arquivo.")),
        }
    }
}

fn list_items(items: &[String], count: i64) -> String {
    let shown: Vec<&str> = items.iter().take(3).map(String::as_str).collect();
    let rest = count - shown.len() as i64;
    match rest {
        r if r > 0 => format!("{} e mais {r}.", shown.join(", ")),
        _ => format!("{}.", shown.join(", ")),
    }
}

fn clip(text: &str) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() <= MAX_TEXT_CHARS {
        return clean.to_string();
    }
    let mut out: String = clean.chars().take(MAX_TEXT_CHARS - 1).collect();
    out.push('…');
    out
}

/// Um evento: a chave estável que o identifica e, quando houver, o capítulo e o nome que ele traz.
#[derive(Debug, Clone)]
pub struct Event {
    pub key: String,
    pub chapter_id: Option<i64>,
    pub item: Option<String>,
}

impl Event {
    pub fn new(key: impl Into<String>) -> Self {
        Event { key: key.into(), chapter_id: None, item: None }
    }

    pub fn item(mut self, item: impl Into<String>) -> Self {
        self.item = Some(item.into());
        self
    }

    pub fn chapter(mut self, id: i64) -> Self {
        self.chapter_id = Some(id);
        self
    }
}

/// Uma notificação a criar. `events` precisa ter pelo menos um evento ainda não visto.
#[derive(Debug, Clone)]
pub struct NewNotification {
    pub kind: Kind,
    pub subject: String,
    pub series_id: Option<i64>,
    pub comic_id: Option<i64>,
    pub extra: Value,
    pub events: Vec<Event>,
    /// Eventos do mesmo grupo dentro deste intervalo somam na notificação ainda não lida.
    pub group_window_ms: Option<i64>,
}

impl NewNotification {
    pub fn new(kind: Kind) -> Self {
        NewNotification { kind, subject: String::new(), series_id: None, comic_id: None, extra: json!({}), events: Vec::new(), group_window_ms: None }
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = subject.into();
        self
    }

    pub fn series(mut self, id: i64) -> Self {
        self.series_id = Some(id);
        self
    }

    pub fn comic(mut self, id: i64) -> Self {
        self.comic_id = Some(id);
        self
    }

    pub fn extra(mut self, extra: Value) -> Self {
        self.extra = extra;
        self
    }

    pub fn event(mut self, event: Event) -> Self {
        self.events.push(event);
        self
    }

    pub fn grouped(mut self, window_ms: i64) -> Self {
        self.group_window_ms = Some(window_ms);
        self
    }

    fn group_key(&self) -> Option<String> {
        self.group_window_ms.map(|_| format!("{}:{}", self.kind.as_str(), self.series_id.map_or_else(String::new, |id| id.to_string())))
    }
}

/// O que o sistema operacional deve mostrar. Sem o nome de obras +18.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NativeNotice {
    pub id: i64,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOutcome {
    Shown,
    /// A janela está em primeiro plano: a central já mostra o aviso.
    Skipped,
    Denied,
    Failed,
}

pub type Notifier = Arc<dyn Fn(&NativeNotice) -> NativeOutcome + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    pub id: i64,
    /// Somou numa notificação que já existia.
    pub merged: bool,
    pub unread: i64,
    pub native: Option<NativeNotice>,
}

// ---------------------------------------------------------------------------
// Preferências
// ---------------------------------------------------------------------------

fn setting_bool(conn: &Connection, key: &str, default: bool) -> bool {
    library::get_setting(conn, key).ok().flatten().and_then(|v| v.as_bool()).unwrap_or(default)
}

fn native_muted(conn: &Connection, now: i64) -> bool {
    library::get_setting(conn, "notifications.nativeMutedUntil").ok().flatten().and_then(|v| v.as_i64()).is_some_and(|until| until > now)
}

fn native_notice(conn: &Connection, n: &NewNotification, id: i64, title: &str, body: &str, now: i64) -> Option<NativeNotice> {
    let (key, default) = n.kind.native_setting()?;
    if !setting_bool(conn, "notifications.native", true) || !setting_bool(conn, key, default) || native_muted(conn, now) {
        return None;
    }
    // Uma obra +18 não aparece pelo nome fora do aplicativo.
    let adult = n.series_id.is_some_and(|series| {
        conn.query_row("SELECT adult = 1 OR classification <> 'known' FROM online_series WHERE id = ?1", [series], |r| r.get::<_, bool>(0)).unwrap_or(true)
    });
    if adult && n.kind == Kind::ChaptersNew {
        return Some(NativeNotice { id, title: "Capítulos novos".into(), body: "Uma obra que você acompanha recebeu capítulos novos.".into() });
    }
    if adult {
        return Some(NativeNotice { id, title: title.to_string(), body: "Abra o Prancha para ver os detalhes.".into() });
    }
    Some(NativeNotice { id, title: title.to_string(), body: body.to_string() })
}

// ---------------------------------------------------------------------------
// Gravação
// ---------------------------------------------------------------------------

/// Grava o evento numa transação. `None`: tudo já tinha sido anunciado, ou a categoria está desligada.
pub fn record(conn: &Connection, n: &NewNotification, now: i64) -> AppResult<Option<Recorded>> {
    let tx = conn.unchecked_transaction()?;
    // A chave fica registrada mesmo com a categoria desligada: ligar de novo não traz eventos antigos.
    let mut fresh = Vec::new();
    for event in &n.events {
        if tx.execute("INSERT OR IGNORE INTO notification_events (dedup_key, created_at) VALUES (?1, ?2)", params![event.key, now])? > 0 {
            fresh.push(event);
        }
    }
    if fresh.is_empty() || !setting_bool(&tx, n.kind.setting(), true) {
        tx.commit()?;
        return Ok(None);
    }
    let group = n.group_key();
    let existing: Option<(i64, String)> = match (&group, n.group_window_ms) {
        (Some(key), Some(window)) => tx
            .query_row(
                "SELECT id, payload FROM notifications WHERE group_key = ?1 AND read_at IS NULL AND created_at >= ?2 ORDER BY created_at DESC, id DESC LIMIT 1",
                params![key, now - window],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?,
        _ => None,
    };
    let new_items: Vec<String> = fresh.iter().filter_map(|e| e.item.as_deref().map(clip)).collect();
    let subject = clip(&n.subject);
    let (id, merged, title, body) = match existing {
        Some((id, payload)) => {
            let mut payload: Value = serde_json::from_str(&payload).unwrap_or_else(|_| json!({}));
            let count = payload.get("count").and_then(Value::as_i64).unwrap_or(1) + fresh.len() as i64;
            let mut items: Vec<String> = payload.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
            items.extend(new_items);
            items.truncate(MAX_ITEMS);
            let subject = payload.get("subject").and_then(Value::as_str).map(str::to_string).unwrap_or(subject);
            payload["count"] = json!(count);
            payload["items"] = json!(items);
            let (title, body) = n.kind.compose(count, &subject, &items, &payload);
            tx.execute(
                "UPDATE notifications SET title = ?2, body = ?3, payload = ?4, updated_at = ?5, action = ?6 WHERE id = ?1",
                params![id, clip(&title), clip(&body), payload.to_string(), now, n.kind.actions(count)[0].as_str()],
            )?;
            (id, true, title, body)
        }
        None => {
            let count = fresh.len() as i64;
            let mut items = new_items;
            items.truncate(MAX_ITEMS);
            let mut payload = if n.extra.is_object() { n.extra.clone() } else { json!({}) };
            payload["count"] = json!(count);
            payload["items"] = json!(items);
            payload["subject"] = json!(subject);
            let (title, body) = n.kind.compose(count, &subject, &items, &payload);
            let chapter = fresh.iter().find_map(|e| e.chapter_id);
            tx.execute(
                "INSERT INTO notifications (category, kind, title, body, series_id, chapter_id, comic_id, action, payload, group_key, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![
                    n.kind.category().as_str(),
                    n.kind.as_str(),
                    clip(&title),
                    clip(&body),
                    n.series_id,
                    chapter,
                    n.comic_id,
                    n.kind.actions(count)[0].as_str(),
                    payload.to_string(),
                    group,
                    now
                ],
            )?;
            (tx.last_insert_rowid(), false, title, body)
        }
    };
    for event in &fresh {
        tx.execute("UPDATE notification_events SET notification_id = ?2 WHERE dedup_key = ?1", params![event.key, id])?;
        if let Some(chapter) = event.chapter_id {
            tx.execute("INSERT OR IGNORE INTO notification_chapters (notification_id, chapter_id) SELECT ?1, id FROM online_chapters WHERE id = ?2", params![id, chapter])?;
        }
    }
    let native = if merged { None } else { native_notice(&tx, n, id, &clip(&title), &clip(&body), now) };
    let unread = unread_count(&tx)?;
    tx.commit()?;
    Ok(Some(Recorded { id, merged, unread, native }))
}

/// Grava, avisa a interface e, quando couber, o sistema operacional. Pode ser chamada com o banco em uso.
pub fn publish_with(core: &Core, conn: &Connection, n: &NewNotification) -> Option<Recorded> {
    let recorded = match record(conn, n, now_ms()) {
        Ok(r) => r?,
        Err(e) => {
            log::warn!("notificação {} não gravada: {e}", n.kind.as_str());
            return None;
        }
    };
    (core.emit)(EVENT_CHANGED, json!({ "unread": recorded.unread, "created": recorded.id }));
    if let (Some(notice), Some(notifier)) = (&recorded.native, core.notifier.get()) {
        let outcome = notifier(notice);
        if matches!(outcome, NativeOutcome::Denied | NativeOutcome::Failed) {
            log::info!("notificação do sistema não exibida: {outcome:?}");
        }
    }
    Some(recorded)
}

pub fn publish(core: &Core, n: &NewNotification) -> Option<Recorded> {
    publish_with(core, &core.db(), n)
}

/// Avisa a interface de uma alteração feita por ela mesma: só a contagem muda, a lista já foi ajustada.
pub fn changed(core: &Core, unread: i64) {
    (core.emit)(EVENT_CHANGED, json!({ "unread": unread }));
}

// ---------------------------------------------------------------------------
// Eventos de cada área
// ---------------------------------------------------------------------------

/// Capítulos que chegam juntos numa obra viram uma notificação; os que chegarem logo depois somam nela.
pub const CHAPTERS_WINDOW_MS: i64 = 30 * 60 * 1000;
pub const DOWNLOADS_WINDOW_MS: i64 = 30 * 60 * 1000;
pub const SYNC_WINDOW_MS: i64 = 6 * 60 * 60 * 1000;

/// Capítulos recém-descobertos de uma obra acompanhada. A chave usa o identificador da fonte, que não
/// muda entre sincronizações, então o mesmo capítulo nunca é anunciado duas vezes.
pub fn chapters_discovered(conn: &Connection, series_id: i64, chapter_ids: &[i64]) -> AppResult<Option<NewNotification>> {
    if chapter_ids.is_empty() {
        return Ok(None);
    }
    let series: Option<(String, bool)> = conn
        .query_row("SELECT title, following = 1 FROM online_series WHERE id = ?1", [series_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((title, following)) = series else { return Ok(None) };
    if !following {
        return Ok(None);
    }
    let placeholders = vec!["?"; chapter_ids.len()].join(",");
    let mut statement = conn.prepare(&format!(
        "SELECT id, remote_id, title FROM online_chapters WHERE series_id = ? AND id IN ({placeholders}) ORDER BY sort_order DESC, id"
    ))?;
    let mut values: Vec<i64> = vec![series_id];
    values.extend_from_slice(chapter_ids);
    let rows = statement.query_map(params_from_iter(values), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    let mut n = NewNotification::new(Kind::ChaptersNew).subject(title).series(series_id).grouped(CHAPTERS_WINDOW_MS);
    for row in rows {
        let (id, remote, chapter_title) = row?;
        n = n.event(Event::new(format!("chapter:new:{series_id}:{remote}")).chapter(id).item(chapter_title));
    }
    Ok((!n.events.is_empty()).then_some(n))
}

// ---------------------------------------------------------------------------
// Consulta
// ---------------------------------------------------------------------------

/// A notificação de uma obra que o filtro +18 esconde também fica escondida, e não conta na badge.
fn visible_sql() -> String {
    format!("(n.series_id IS NULL OR EXISTS (SELECT 1 FROM online_series o WHERE o.id = n.series_id AND ({SERIES_SAFE_SQL} OR {ADULT_ALLOWED_SQL})))")
}

pub fn unread_count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(&format!("SELECT count(*) FROM notifications n WHERE n.read_at IS NULL AND {}", visible_sql()), [], |r| r.get(0))?)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub created_at: i64,
    pub id: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListQuery {
    /// `unread` ou `read`; vazio é tudo.
    pub state: Option<String>,
    pub category: Option<String>,
    pub series_id: Option<i64>,
    pub before: Option<Cursor>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationView {
    pub id: i64,
    pub category: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub series_id: Option<i64>,
    pub series_has_cover: bool,
    pub chapter_id: Option<i64>,
    pub chapter_ids: Vec<i64>,
    pub comic_id: Option<i64>,
    pub comic_cover_rev: Option<i64>,
    pub actions: Vec<Action>,
    pub count: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub read_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPage {
    pub items: Vec<NotificationView>,
    pub next_cursor: Option<Cursor>,
    pub unread: i64,
}

const VIEW_SELECT: &str = "SELECT n.id, n.category, n.kind, n.title, n.body, n.series_id, (SELECT o.cover_url IS NOT NULL FROM online_series o WHERE o.id = n.series_id),
    n.chapter_id, (SELECT json_group_array(nc.chapter_id) FROM notification_chapters nc WHERE nc.notification_id = n.id), n.comic_id,
    (SELECT c.cover_rev FROM comics c WHERE c.id = n.comic_id AND c.cover_kind <> 'placeholder'), n.action, n.payload, n.created_at, n.updated_at, n.read_at
    FROM notifications n";

fn view_from_row(r: &Row) -> rusqlite::Result<NotificationView> {
    let kind: String = r.get(2)?;
    let payload: Value = serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_else(|_| json!({}));
    let count = payload.get("count").and_then(Value::as_i64).unwrap_or(1);
    let primary = r.get::<_, Option<String>>(11)?.as_deref().and_then(Action::parse);
    let mut actions: Vec<Action> = Kind::parse(&kind).map(|k| k.actions(count).to_vec()).unwrap_or_default();
    if let Some(primary) = primary {
        actions.retain(|a| *a != primary);
        actions.insert(0, primary);
    }
    Ok(NotificationView {
        id: r.get(0)?,
        category: r.get(1)?,
        kind,
        title: r.get(3)?,
        body: r.get(4)?,
        series_id: r.get(5)?,
        series_has_cover: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
        chapter_id: r.get(7)?,
        chapter_ids: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
        comic_id: r.get(9)?,
        comic_cover_rev: r.get(10)?,
        actions,
        count,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
        read_at: r.get(15)?,
    })
}

/// Uma página, da mais recente para a mais antiga. `before` continua de onde a anterior parou.
pub fn list(conn: &Connection, query: &ListQuery) -> AppResult<NotificationPage> {
    let limit = query.limit.unwrap_or(30).clamp(1, MAX_PAGE);
    let mut filters = vec![visible_sql()];
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    match query.state.as_deref() {
        Some("unread") => filters.push("n.read_at IS NULL".into()),
        Some("read") => filters.push("n.read_at IS NOT NULL".into()),
        _ => {}
    }
    if let Some(category) = query.category.as_deref() {
        let category = Category::parse(category).ok_or_else(|| AppError::user("invalid", "Categoria de notificação desconhecida."))?;
        filters.push("n.category = ?".into());
        values.push(category.as_str().to_string().into());
    }
    if let Some(series) = query.series_id {
        filters.push("n.series_id = ?".into());
        values.push(series.into());
    }
    if let Some(cursor) = query.before {
        filters.push("(n.created_at < ? OR (n.created_at = ? AND n.id < ?))".into());
        values.push(cursor.created_at.into());
        values.push(cursor.created_at.into());
        values.push(cursor.id.into());
    }
    values.push((limit + 1).into());
    let sql = format!("{VIEW_SELECT} WHERE {} ORDER BY n.created_at DESC, n.id DESC LIMIT ?", filters.join(" AND "));
    let mut statement = conn.prepare(&sql)?;
    let mut items: Vec<NotificationView> = statement.query_map(params_from_iter(values), view_from_row)?.collect::<Result<_, _>>()?;
    let next_cursor = if items.len() as i64 > limit {
        items.truncate(limit as usize);
        items.last().map(|n| Cursor { created_at: n.created_at, id: n.id })
    } else {
        None
    };
    Ok(NotificationPage { items, next_cursor, unread: unread_count(conn)? })
}

// ---------------------------------------------------------------------------
// Alterações
// ---------------------------------------------------------------------------

fn id_list(ids: &[i64]) -> AppResult<String> {
    if ids.is_empty() || ids.len() > 1000 {
        return Err(AppError::user("invalid", "Escolha de 1 a 1000 notificações."));
    }
    Ok(vec!["?"; ids.len()].join(","))
}

/// Ler ou deixar de ler muda só a notificação. O evento original não volta a acontecer.
pub fn set_read(conn: &Connection, ids: &[i64], read: bool, now: i64) -> AppResult<i64> {
    let placeholders = id_list(ids)?;
    let sql = if read {
        format!("UPDATE notifications SET read_at = ? WHERE read_at IS NULL AND id IN ({placeholders})")
    } else {
        format!("UPDATE notifications SET read_at = NULL WHERE read_at IS NOT NULL AND id IN ({placeholders})")
    };
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if read {
        values.push(now.into());
    }
    values.extend(ids.iter().map(|id| (*id).into()));
    conn.execute(&sql, params_from_iter(values))?;
    unread_count(conn)
}

pub fn mark_all_read(conn: &Connection, category: Option<&str>, now: i64) -> AppResult<i64> {
    match category {
        Some(c) => {
            let category = Category::parse(c).ok_or_else(|| AppError::user("invalid", "Categoria de notificação desconhecida."))?;
            conn.execute("UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL AND category = ?2", params![now, category.as_str()])?
        }
        None => conn.execute("UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL", [now])?,
    };
    unread_count(conn)
}

/// Remove só as notificações. Capítulos, obras e progresso ficam como estão.
pub fn remove(conn: &Connection, ids: &[i64]) -> AppResult<i64> {
    let placeholders = id_list(ids)?;
    conn.execute(&format!("DELETE FROM notifications WHERE id IN ({placeholders})"), params_from_iter(ids.iter()))?;
    unread_count(conn)
}

pub fn clear_read(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM notifications WHERE read_at IS NOT NULL", [])?)
}

/// Retenção, a cada início.
pub fn prune(conn: &Connection, now: i64) -> AppResult<usize> {
    let mut removed = conn.execute("DELETE FROM notifications WHERE (read_at IS NOT NULL AND created_at < ?1) OR created_at < ?2", params![now - KEEP_READ_MS, now - KEEP_ANY_MS])?;
    removed += conn.execute(
        "DELETE FROM notifications WHERE id IN (SELECT id FROM notifications ORDER BY created_at DESC, id DESC LIMIT -1 OFFSET ?1)",
        [KEEP_MAX],
    )?;
    conn.execute("DELETE FROM notification_events WHERE created_at < ?1", [now - KEEP_EVENTS_MS])?;
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Ações
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Target {
    Series { id: i64 },
    Chapter { id: i64 },
    Comic { id: i64 },
    Downloads,
    Updates,
    Library,
    RetrySync,
}

fn missing(message: &str) -> AppError {
    AppError::user("notification_target_missing", message)
}

/// Confere se o destino ainda existe antes de a interface navegar. Nada aqui altera dados.
pub fn resolve(conn: &Connection, id: i64, action: Option<Action>) -> AppResult<Target> {
    let row: Option<(String, Option<i64>, Option<i64>, Option<i64>, Option<String>, String)> = conn
        .query_row(
            &format!("SELECT kind, series_id, chapter_id, comic_id, action, payload FROM notifications n WHERE n.id = ?1 AND {}", visible_sql()),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    let (kind, series, chapter, comic, primary, payload) = row.ok_or_else(|| missing("Esta notificação não existe mais."))?;
    let kind = Kind::parse(&kind).ok_or_else(|| missing("Esta notificação não tem uma ação disponível."))?;
    let count = serde_json::from_str::<Value>(&payload).ok().and_then(|p| p.get("count").and_then(Value::as_i64)).unwrap_or(1);
    let allowed = kind.actions(count);
    let action = match action {
        Some(a) if allowed.contains(&a) || primary.as_deref() == Some(a.as_str()) => a,
        Some(_) => return Err(AppError::user("invalid", "Esta ação não pertence a esta notificação.")),
        None => primary.as_deref().and_then(Action::parse).unwrap_or(allowed[0]),
    };
    let online = || conn.query_row("SELECT EXISTS (SELECT 1 FROM settings WHERE key = 'plugins.online' AND value = 'true')", [], |r| r.get::<_, bool>(0));
    match action {
        Action::OpenSeries => {
            let id = series.ok_or_else(|| missing("A obra desta notificação não está mais registrada."))?;
            if !online()? {
                return Err(AppError::user("plugin_off", "Ative a leitura online em Plugins para abrir esta obra."));
            }
            Ok(Target::Series { id })
        }
        Action::OpenChapter => {
            // O capítulo principal, ou o primeiro anunciado que ainda existe.
            let id = match chapter {
                Some(id) => Some(id),
                None => conn.query_row("SELECT min(chapter_id) FROM notification_chapters WHERE notification_id = ?1", [id], |r| r.get::<_, Option<i64>>(0))?,
            };
            let id = id.ok_or_else(|| missing("Este capítulo não está mais disponível na fonte."))?;
            if !online()? {
                return Err(AppError::user("plugin_off", "Ative a leitura online em Plugins para abrir este capítulo."));
            }
            Ok(Target::Chapter { id })
        }
        Action::OpenComic => {
            let id = comic.ok_or_else(|| missing("Esta HQ não está mais na biblioteca."))?;
            Ok(Target::Comic { id })
        }
        Action::OpenDownloads => {
            if !online()? {
                return Err(AppError::user("plugin_off", "Ative a leitura online em Plugins para ver os downloads."));
            }
            Ok(Target::Downloads)
        }
        Action::OpenUpdates => Ok(Target::Updates),
        Action::OpenLibrary => Ok(Target::Library),
        Action::RetrySync => Ok(Target::RetrySync),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn conn() -> Connection {
        let conn = db::open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('plugins.online', 'true');
             INSERT INTO online_series (id, extension_id, source_id, remote_id, title, following, classification, created_at, updated_at) VALUES (1, 'e', 's', 'op', 'One Piece', 1, 'known', 1, 1);
             INSERT INTO online_series (id, extension_id, source_id, remote_id, title, following, classification, created_at, updated_at) VALUES (2, 'e', 's', 'nar', 'Naruto', 1, 'known', 1, 1);
             INSERT INTO online_series (id, extension_id, source_id, remote_id, title, following, classification, created_at, updated_at) VALUES (3, 'e', 's', 'x', 'Não acompanhada', 0, 'known', 1, 1);",
        )
        .unwrap();
        conn
    }

    fn chapter(conn: &Connection, series: i64, remote: &str, order: i64) -> i64 {
        conn.execute("INSERT INTO online_chapters (series_id, remote_id, title, sort_order) VALUES (?1, ?2, ?3, ?4)", params![series, remote, format!("Capítulo {remote}"), order]).unwrap();
        conn.last_insert_rowid()
    }

    fn discover(conn: &Connection, series: i64, ids: &[i64], now: i64) -> Option<Recorded> {
        let n = chapters_discovered(conn, series, ids).unwrap()?;
        record(conn, &n, now).unwrap()
    }

    fn page(conn: &Connection, query: ListQuery) -> NotificationPage {
        list(conn, &query).unwrap()
    }

    fn update(kind: Kind, version: &str) -> NewNotification {
        NewNotification::new(kind).extra(json!({ "version": version })).event(Event::new(format!("{}:{version}", kind.as_str())))
    }

    #[test]
    fn new_chapters_become_one_unread_notification_per_series() {
        let conn = conn();
        let a = chapter(&conn, 1, "1100", 1100);
        let b = chapter(&conn, 1, "1101", 1101);
        let c = chapter(&conn, 1, "1102", 1102);
        let r = discover(&conn, 1, &[a, b, c], 1000).unwrap();
        assert!(!r.merged);
        assert_eq!(r.unread, 1, "a badge conta notificações, não capítulos");
        let items = page(&conn, ListQuery::default()).items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "One Piece recebeu 3 capítulos novos");
        assert_eq!(items[0].body, "Capítulo 1102, Capítulo 1101, Capítulo 1100.");
        assert_eq!(items[0].chapter_ids.len(), 3);
        assert_eq!(items[0].actions[0], Action::OpenSeries);
        assert_eq!(items[0].read_at, None);
    }

    #[test]
    fn one_chapter_opens_the_chapter() {
        let conn = conn();
        let a = chapter(&conn, 1, "42", 42);
        discover(&conn, 1, &[a], 1000).unwrap();
        let item = &page(&conn, ListQuery::default()).items[0];
        assert_eq!(item.title, "Capítulo novo em One Piece");
        assert_eq!(item.body, "Capítulo 42 está disponível.");
        assert_eq!(resolve(&conn, item.id, None).unwrap(), Target::Chapter { id: a });
        assert_eq!(resolve(&conn, item.id, Some(Action::OpenSeries)).unwrap(), Target::Series { id: 1 });
    }

    #[test]
    fn a_repeated_sync_does_not_duplicate_and_later_chapters_join_the_unread_one() {
        let conn = conn();
        let a = chapter(&conn, 1, "1", 1);
        discover(&conn, 1, &[a], 1000).unwrap();
        assert_eq!(discover(&conn, 1, &[a], 2000), None, "o mesmo capítulo descoberto de novo");
        let b = chapter(&conn, 1, "2", 2);
        let r = discover(&conn, 1, &[a, b], 3000).unwrap();
        assert!(r.merged);
        assert_eq!(r.unread, 1);
        let items = page(&conn, ListQuery::default()).items;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].count, 2);
        assert_eq!(items[0].title, "One Piece recebeu 2 capítulos novos");
        assert_eq!(items[0].actions[0], Action::OpenSeries, "com dois capítulos, a ação principal passa a ser a obra");
    }

    #[test]
    fn chapters_after_the_window_or_after_reading_get_a_new_notification() {
        let conn = conn();
        let a = chapter(&conn, 1, "1", 1);
        let first = discover(&conn, 1, &[a], 1000).unwrap();
        set_read(&conn, &[first.id], true, 1500).unwrap();
        let b = chapter(&conn, 1, "2", 2);
        let second = discover(&conn, 1, &[b], 2000).unwrap();
        assert!(!second.merged, "a lida não recebe novidades escondidas");
        let c = chapter(&conn, 1, "3", 3);
        let third = discover(&conn, 1, &[c], 2000 + CHAPTERS_WINDOW_MS + 1).unwrap();
        assert!(!third.merged);
        assert_eq!(third.unread, 2);
    }

    #[test]
    fn several_series_get_separate_notifications_and_unfollowed_ones_none() {
        let conn = conn();
        let a = chapter(&conn, 1, "1", 1);
        let b = chapter(&conn, 2, "1", 1);
        let c = chapter(&conn, 3, "1", 1);
        discover(&conn, 1, &[a], 1000).unwrap();
        let r = discover(&conn, 2, &[b], 1000).unwrap();
        assert_eq!(r.unread, 2);
        assert!(chapters_discovered(&conn, 3, &[c]).unwrap().is_none());
        assert!(chapters_discovered(&conn, 1, &[b]).unwrap().is_none(), "capítulo de outra obra não entra");
    }

    #[test]
    fn reading_a_notification_does_not_touch_chapters_or_progress() {
        let conn = conn();
        let a = chapter(&conn, 1, "1", 1);
        conn.execute("INSERT INTO chapter_progress (chapter_id, page_index, page_count, updated_at) VALUES (?1, 2, 10, 1)", [a]).unwrap();
        let r = discover(&conn, 1, &[a], 1000).unwrap();
        let before: String = conn.query_row("SELECT json_array(c.title, c.comic_id, q.page_index) FROM online_chapters c JOIN chapter_progress q ON q.chapter_id = c.id WHERE c.id = ?1", [a], |r| r.get(0)).unwrap();
        assert_eq!(set_read(&conn, &[r.id], true, 2000).unwrap(), 0);
        assert_eq!(set_read(&conn, &[r.id], false, 3000).unwrap(), 1);
        assert_eq!(remove(&conn, &[r.id]).unwrap(), 0);
        let after: String = conn.query_row("SELECT json_array(c.title, c.comic_id, q.page_index) FROM online_chapters c JOIN chapter_progress q ON q.chapter_id = c.id WHERE c.id = ?1", [a], |r| r.get(0)).unwrap();
        assert_eq!(before, after);
        assert_eq!(discover(&conn, 1, &[a], 4000), None, "remover não faz o evento voltar");
    }

    #[test]
    fn marking_unread_does_not_create_the_event_again() {
        let conn = conn();
        let r = record(&conn, &update(Kind::UpdateAvailable, "1.0.0"), 1000).unwrap().unwrap();
        set_read(&conn, &[r.id], true, 2000).unwrap();
        set_read(&conn, &[r.id], false, 3000).unwrap();
        assert_eq!(record(&conn, &update(Kind::UpdateAvailable, "1.0.0"), 4000).unwrap(), None);
        assert_eq!(page(&conn, ListQuery::default()).items.len(), 1);
        assert!(record(&conn, &update(Kind::UpdateAvailable, "1.1.0"), 5000).unwrap().is_some(), "outra versão é outro evento");
    }

    #[test]
    fn counts_stay_right_through_every_change() {
        let conn = conn();
        let mut ids = Vec::new();
        for v in 0..120 {
            ids.push(record(&conn, &update(Kind::UpdateAvailable, &format!("1.{v}.0")), 1000 + v).unwrap().unwrap().id);
        }
        assert_eq!(unread_count(&conn).unwrap(), 120);
        assert_eq!(set_read(&conn, &ids[..20], true, 5000).unwrap(), 100);
        assert_eq!(set_read(&conn, &ids[..20], true, 5000).unwrap(), 100, "marcar de novo não muda a conta");
        assert_eq!(remove(&conn, &ids[20..30]).unwrap(), 90);
        assert_eq!(remove(&conn, &ids[..10]).unwrap(), 90, "remover lidas não mexe nas não lidas");
        assert_eq!(mark_all_read(&conn, None, 6000).unwrap(), 0);
    }

    #[test]
    fn list_pages_newest_first_and_filters_by_state_and_category() {
        let conn = conn();
        for v in 0..7 {
            record(&conn, &update(Kind::UpdateAvailable, &format!("2.{v}.0")), 100 * v).unwrap();
        }
        let a = chapter(&conn, 1, "1", 1);
        let chapters = discover(&conn, 1, &[a], 50).unwrap();
        set_read(&conn, &[chapters.id], true, 900).unwrap();
        let first = page(&conn, ListQuery { limit: Some(3), ..Default::default() });
        let times: Vec<i64> = first.items.iter().map(|n| n.created_at).collect();
        assert_eq!(times, vec![600, 500, 400]);
        let second = page(&conn, ListQuery { limit: Some(3), before: first.next_cursor, ..Default::default() });
        assert_eq!(second.items.iter().map(|n| n.created_at).collect::<Vec<_>>(), vec![300, 200, 100]);
        let third = page(&conn, ListQuery { limit: Some(3), before: second.next_cursor, ..Default::default() });
        assert_eq!(third.items.iter().map(|n| n.created_at).collect::<Vec<_>>(), vec![50, 0]);
        assert_eq!(third.next_cursor, None);
        assert_eq!(page(&conn, ListQuery { state: Some("read".into()), ..Default::default() }).items.len(), 1);
        assert_eq!(page(&conn, ListQuery { state: Some("unread".into()), ..Default::default() }).items.len(), 7);
        assert_eq!(page(&conn, ListQuery { category: Some("chapters".into()), ..Default::default() }).items.len(), 1);
        assert_eq!(page(&conn, ListQuery { category: Some("downloads".into()), ..Default::default() }).items.len(), 0);
        assert_eq!(page(&conn, ListQuery { series_id: Some(1), ..Default::default() }).items.len(), 1);
        assert!(list(&conn, &ListQuery { category: Some("qualquer".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn a_disabled_category_stores_nothing_and_does_not_bring_old_events_back() {
        let conn = conn();
        library::set_setting(&conn, "notifications.updates", &json!(false)).unwrap();
        assert_eq!(record(&conn, &update(Kind::UpdateAvailable, "3.0.0"), 1).unwrap(), None);
        library::set_setting(&conn, "notifications.updates", &json!(true)).unwrap();
        assert_eq!(record(&conn, &update(Kind::UpdateAvailable, "3.0.0"), 2).unwrap(), None);
        assert_eq!(unread_count(&conn).unwrap(), 0);
    }

    #[test]
    fn native_notices_follow_the_preferences() {
        let conn = conn();
        let r = record(&conn, &update(Kind::UpdateAvailable, "4.0.0"), 1).unwrap().unwrap();
        assert_eq!(r.native.as_ref().map(|n| n.title.as_str()), Some("Nova versão disponível"));
        let r = record(&conn, &update(Kind::UpdateInstalled, "4.0.0"), 1).unwrap().unwrap();
        assert_eq!(r.native, None, "atualização instalada só aparece na central");

        library::set_setting(&conn, "notifications.native", &json!(false)).unwrap();
        let r = record(&conn, &update(Kind::UpdateAvailable, "4.1.0"), 2).unwrap().unwrap();
        assert_eq!(r.native, None);
        assert_eq!(r.unread, 3, "desligar as do sistema não tira nada da central");

        library::set_setting(&conn, "notifications.native", &json!(true)).unwrap();
        library::set_setting(&conn, "notifications.nativeMutedUntil", &json!(100)).unwrap();
        assert_eq!(record(&conn, &update(Kind::UpdateAvailable, "4.2.0"), 50).unwrap().unwrap().native, None, "silenciado");
        assert!(record(&conn, &update(Kind::UpdateAvailable, "4.3.0"), 150).unwrap().unwrap().native.is_some(), "o silêncio acaba sozinho");

        let download = |key: &str| NewNotification::new(Kind::DownloadCompleted).subject("One Piece").series(1).event(Event::new(key.to_string()).item("Capítulo 1"));
        assert_eq!(record(&conn, &download("d1"), 200).unwrap().unwrap().native, None, "downloads concluídos só para quem pedir");
        library::set_setting(&conn, "notifications.nativeDownloads", &json!(true)).unwrap();
        assert!(record(&conn, &download("d2"), 300 + DOWNLOADS_WINDOW_MS).unwrap().unwrap().native.is_some());
    }

    #[test]
    fn adult_series_stay_out_of_native_text_and_the_filtered_list() {
        let conn = conn();
        conn.execute("UPDATE online_series SET adult = 1 WHERE id = 2", []).unwrap();
        let a = chapter(&conn, 2, "1", 1);
        let r = discover(&conn, 2, &[a], 1).unwrap();
        let native = r.native.unwrap();
        assert!(!native.body.contains("Naruto") && !native.title.contains("Naruto"));
        assert_eq!(r.unread, 0, "com o filtro +18 ligado, não conta");
        assert!(page(&conn, ListQuery::default()).items.is_empty());
        assert!(resolve(&conn, r.id, None).is_err());
        library::set_setting(&conn, "content.adult", &json!(true)).unwrap();
        assert_eq!(page(&conn, ListQuery::default()).items.len(), 1);
    }

    #[test]
    fn actions_check_that_the_target_still_exists() {
        let conn = conn();
        let a = chapter(&conn, 1, "1", 1);
        let r = discover(&conn, 1, &[a], 1).unwrap();
        conn.execute("DELETE FROM online_chapters WHERE id = ?1", [a]).unwrap();
        let err = resolve(&conn, r.id, None).unwrap_err();
        assert_eq!(err.code(), "notification_target_missing");
        assert_eq!(page(&conn, ListQuery::default()).items.len(), 1, "o capítulo sumir não apaga a notificação");
        conn.execute("DELETE FROM online_series WHERE id = 1", []).unwrap();
        assert_eq!(resolve(&conn, r.id, Some(Action::OpenSeries)).unwrap_err().code(), "notification_target_missing");
        assert_eq!(resolve(&conn, r.id, Some(Action::OpenDownloads)).unwrap_err().code(), "invalid", "ação de outro tipo é recusada");
        assert_eq!(resolve(&conn, 999, None).unwrap_err().code(), "notification_target_missing");
        let up = record(&conn, &update(Kind::UpdateAvailable, "5.0.0"), 2).unwrap().unwrap();
        assert_eq!(resolve(&conn, up.id, None).unwrap(), Target::Updates);
        library::set_setting(&conn, "plugins.online", &json!(false)).unwrap();
        let d = record(&conn, &NewNotification::new(Kind::DownloadFailed).subject("X").event(Event::new("df").item("C")), 3).unwrap().unwrap();
        assert_eq!(resolve(&conn, d.id, None).unwrap_err().code(), "plugin_off");
    }

    #[test]
    fn retention_keeps_recent_and_unread_history() {
        let conn = conn();
        let now = 1000 * DAY_MS;
        let old_read = record(&conn, &update(Kind::UpdateAvailable, "6.0.0"), now - 100 * DAY_MS).unwrap().unwrap();
        set_read(&conn, &[old_read.id], true, now - 99 * DAY_MS).unwrap();
        let old_unread = record(&conn, &update(Kind::UpdateAvailable, "6.1.0"), now - 100 * DAY_MS).unwrap().unwrap();
        let ancient = record(&conn, &update(Kind::UpdateAvailable, "6.2.0"), now - 370 * DAY_MS).unwrap().unwrap();
        let recent_read = record(&conn, &update(Kind::UpdateAvailable, "6.3.0"), now - DAY_MS).unwrap().unwrap();
        set_read(&conn, &[recent_read.id], true, now).unwrap();
        assert_eq!(prune(&conn, now).unwrap(), 2);
        let left: Vec<i64> = page(&conn, ListQuery::default()).items.iter().map(|n| n.id).collect();
        assert_eq!(left, vec![recent_read.id, old_unread.id]);
        assert!(!left.contains(&ancient.id));
        assert_eq!(record(&conn, &update(Kind::UpdateAvailable, "6.0.0"), now).unwrap(), None, "a chave continua depois da limpeza");
    }

    #[test]
    fn clearing_read_keeps_unread() {
        let conn = conn();
        let a = record(&conn, &update(Kind::UpdateAvailable, "7.0.0"), 1).unwrap().unwrap();
        record(&conn, &update(Kind::UpdateAvailable, "7.1.0"), 2).unwrap().unwrap();
        set_read(&conn, &[a.id], true, 3).unwrap();
        assert_eq!(clear_read(&conn).unwrap(), 1);
        assert_eq!(page(&conn, ListQuery::default()).items.len(), 1);
    }

    /// Do registro de capítulos (`sources::store_chapters`) até a notificação, como em `open_series`.
    #[test]
    fn synced_chapters_notify_only_what_is_new_and_once() {
        use crate::extensions::{runtime::Chapter, sources};
        let mut conn = conn();
        let ch = |id: &str, order: u32| Chapter { id: id.into(), title: format!("Capítulo {id}"), number: Some(id.into()), published_at: None, order };
        let sync = |conn: &mut Connection, list: &[Chapter], now: i64| {
            let fresh = sources::store_chapters(conn, 1, list).unwrap();
            chapters_discovered(conn, 1, &fresh).unwrap().and_then(|n| record(conn, &n, now).unwrap())
        };
        assert_eq!(sync(&mut conn, &[ch("1", 1), ch("2", 2)], 10), None, "a primeira sincronização é a referência");
        let r = sync(&mut conn, &[ch("1", 1), ch("2", 2), ch("3", 3), ch("4", 4)], 20).unwrap();
        assert_eq!(page(&conn, ListQuery::default()).items[0].title, "One Piece recebeu 2 capítulos novos");
        assert_eq!(sync(&mut conn, &[ch("1", 1), ch("2", 2), ch("3", 3), ch("4", 4)], 30), None, "sincronizar de novo não repete");
        set_read(&conn, &[r.id], true, 40).unwrap();
        let acknowledged: i64 = conn.query_row("SELECT count(*) FROM online_chapters WHERE series_id = 1 AND acknowledged_at IS NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(acknowledged, 2, "ler a notificação não reconhece a novidade dos capítulos");
        let next = sync(&mut conn, &[ch("1", 1), ch("2", 2), ch("3", 3), ch("4", 4), ch("5", 5)], 50).unwrap();
        assert!(!next.merged);
        assert_eq!(unread_count(&conn).unwrap(), 1);
    }

    #[test]
    fn texts_are_clipped_and_free_of_control_characters() {
        let conn = conn();
        let long = "A".repeat(500);
        let n = NewNotification::new(Kind::DownloadFailed).subject(format!("Obra\u{7}{long}")).event(Event::new("x").item("Cap"));
        record(&conn, &n, 1).unwrap();
        let item = &page(&conn, ListQuery::default()).items[0];
        assert!(item.body.chars().count() <= MAX_TEXT_CHARS);
        assert!(!item.body.contains('\u{7}'));
    }
}
