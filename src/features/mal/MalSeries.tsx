// A associação da obra com o MyAnimeList, na página da obra: qual obra do MAL, como está a sincronização e o
// caminho para corrigir. Só aparece com uma conta conectada; sem ela, a página fica como sempre foi.
import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { AlertCircle, Check, ExternalLink, Link2, Link2Off, RefreshCw, Search } from "lucide-react";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Spinner } from "../../components/Feedback";
import { TextField } from "../../components/Form";
import { useToast } from "../../components/Toast";
import { cn } from "../../components/cn";
import { candidateDetails, syncLabel, type MalCandidate, type MalSeriesStatus } from "../../domain/mal";
import { api, errorMessage } from "../../services/api";
import { onMalChanged } from "../../services/events";
import { Fact } from "../online/parts";

function CandidateRow({ candidate, current, busy, onChoose }: { candidate: MalCandidate; current: boolean; busy: boolean; onChoose: () => void }) {
  return (
    <li className="flex items-start gap-3 border-b border-border py-2.5 last:border-b-0">
      <div className="min-w-0 flex-1">
        <p className="font-medium break-words">{candidate.title}</p>
        {candidate.alternative.length > 0 && <p className="text-sm break-words text-muted">{candidate.alternative.join(" · ")}</p>}
        <p className="text-sm text-muted">{candidateDetails(candidate) || "Sem detalhes no MAL"}</p>
      </div>
      {current ? (
        <span className="flex h-8 items-center gap-1 text-sm text-success">
          <Check size={14} aria-hidden /> Associada
        </span>
      ) : (
        <Button size="sm" disabled={busy} onClick={onChoose} aria-label={`Associar a ${candidate.title}`}>
          Associar
        </Button>
      )}
    </li>
  );
}

function LinkDialog({ seriesId, title, status, onClose, onChanged }: { seriesId: number; title: string; status: MalSeriesStatus; onClose: () => void; onChanged: (s: MalSeriesStatus) => void }) {
  const toast = useToast();
  const [query, setQuery] = useState(title);
  const [results, setResults] = useState<MalCandidate[]>(status.candidates);
  const [searched, setSearched] = useState(status.candidates.length > 0);
  const [searching, setSearching] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const search = useCallback(
    async (text: string) => {
      setSearching(true);
      setError(null);
      try {
        setResults(await api.malSearch(seriesId, text));
        setSearched(true);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        setSearching(false);
      }
    },
    [seriesId],
  );

  // Sem sugestões guardadas, a pesquisa pelo título já começa feita.
  const started = useRef(false);
  useEffect(() => {
    if (started.current || status.candidates.length > 0) return;
    started.current = true;
    void search(title);
  }, [search, status.candidates.length, title]);

  const act = async (action: () => Promise<MalSeriesStatus>, done: string) => {
    setBusy(true);
    try {
      onChanged(await action());
      toast.success(done);
      onClose();
    } catch (e) {
      toast.error("Não foi possível mudar a associação", errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    void search(query);
  };

  return (
    <Dialog
      open
      size="lg"
      title="Associar ao MyAnimeList"
      description="Escolha a obra do MyAnimeList que corresponde a esta. O progresso só é enviado para a obra associada."
      onClose={onClose}
      footer={
        <>
          {status.state !== "none" && (
            <Button variant="ghost" icon={Link2Off} disabled={busy} onClick={() => void act(() => api.malUnlink(seriesId), "A obra ficou sem associação")}>
              Não está no MyAnimeList
            </Button>
          )}
          <Button onClick={onClose}>Fechar</Button>
        </>
      }
    >
      <form onSubmit={submit} className="flex items-end gap-2" role="search">
        <TextField className="flex-1" label="Título, número ou endereço da obra no MAL" value={query} onChange={(e) => setQuery(e.target.value)} type="search" data-autofocus />
        <Button type="submit" icon={Search} loading={searching}>
          Pesquisar
        </Button>
      </form>
      {error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {error}
        </p>
      )}
      <div className="mt-3" aria-busy={searching}>
        {searching && results.length === 0 ? (
          <Spinner label="Pesquisando no MyAnimeList…" />
        ) : results.length > 0 ? (
          <ul aria-label="Obras encontradas no MyAnimeList">
            {results.map((c) => (
              <CandidateRow key={c.malId} candidate={c} current={status.link?.malId === c.malId} busy={busy} onChoose={() => void act(() => api.malLink(seriesId, c.malId), `Associada a ${c.title}`)} />
            ))}
          </ul>
        ) : (
          searched && !error && <p className="text-sm text-muted">Nada encontrado. Tente outro título, o número da obra no MAL ou o endereço da página dela.</p>
        )}
      </div>
    </Dialog>
  );
}

/** Linha "MyAnimeList" nos dados da obra. */
export function MalSeriesFact({ seriesId, title }: { seriesId: number; title: string }) {
  const toast = useToast();
  const [status, setStatus] = useState<MalSeriesStatus | null>(null);
  const [looking, setLooking] = useState(true);
  const [editing, setEditing] = useState(false);

  // Ao abrir a obra: procura a associação uma vez (o backend guarda o resultado). Depois só acompanha as mudanças.
  useEffect(() => {
    let alive = true;
    api
      .malSeries(seriesId, true)
      .then((s) => alive && setStatus(s))
      .catch(() => undefined) // sem MAL, a página continua igual
      .finally(() => alive && setLooking(false));
    const off = onMalChanged(() => {
      api
        .malSeries(seriesId, false)
        .then((s) => alive && setStatus(s))
        .catch(() => undefined);
    });
    return () => {
      alive = false;
      off();
    };
  }, [seriesId]);

  const lookAgain = async () => {
    setLooking(true);
    try {
      setStatus(await api.malSeries(seriesId, true));
    } catch (e) {
      toast.error("Não foi possível consultar o MyAnimeList", errorMessage(e));
    } finally {
      setLooking(false);
    }
  };

  if (!status || !status.available || (!status.connected && status.state !== "linked")) return null;

  const link = status.link;
  const sync = syncLabel(status);
  const failed = status.sync?.status === "failed";
  const pending = status.sync?.status === "pending";
  const change = (
    <Button size="sm" variant="ghost" icon={Link2} onClick={() => setEditing(true)}>
      {status.state === "linked" ? "Corrigir" : status.state === "ambiguous" ? "Escolher" : "Associar"}
    </Button>
  );

  let body;
  if (looking && status.state === "unchecked") {
    body = <Spinner size={14} label="Procurando no MyAnimeList…" />;
  } else if (status.state === "linked" && link?.malId) {
    const malId = link.malId;
    body = (
      <>
        <button
          type="button"
          className="inline-flex items-center gap-1 text-left text-accent hover:underline"
          onClick={() => void api.malOpen(malId).catch((e) => toast.error("Não foi possível abrir o navegador", errorMessage(e)))}
        >
          {link.title ?? `Obra ${malId}`}
          <ExternalLink size={13} aria-hidden />
          <span className="sr-only">(abre no navegador)</span>
        </button>
        {sync && (
          <span className={cn("ml-2 inline-flex items-center gap-1 text-sm", failed ? "text-danger" : "text-muted")} data-testid="mal-sync">
            {failed ? <AlertCircle size={13} aria-hidden /> : pending ? <RefreshCw size={13} aria-hidden /> : <Check size={13} aria-hidden />}
            {sync}
          </span>
        )}
        {failed && status.sync?.error && <span className="block text-sm text-muted">{status.sync.error}</span>}
      </>
    );
  } else if (status.state === "ambiguous") {
    body = <span>Mais de uma obra parecida no MAL</span>;
  } else if (status.state === "not_found") {
    body = <span className="text-muted">Não encontrada no MAL</span>;
  } else if (status.state === "none") {
    body = <span className="text-muted">Sem associação</span>;
  } else if (status.state === "error") {
    body = (
      <span className="text-muted">
        {status.error ?? "Não foi possível consultar o MyAnimeList."}{" "}
        <Button size="sm" variant="ghost" icon={RefreshCw} loading={looking} onClick={() => void lookAgain()}>
          Tentar de novo
        </Button>
      </span>
    );
  } else {
    body = <span className="text-muted">Ainda não associada</span>;
  }

  return (
    <Fact label="MyAnimeList">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1" data-testid="mal-series">
        <div className="min-w-0">{body}</div>
        {status.connected && status.state !== "error" && !(looking && status.state === "unchecked") && change}
      </div>
      {editing && <LinkDialog seriesId={seriesId} title={title} status={status} onClose={() => setEditing(false)} onChanged={setStatus} />}
    </Fact>
  );
}
