import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import { ArrowLeft } from "lucide-react";
import { useNav } from "../../app/navigation";
import { Button } from "../../components/Button";
import { ErrorState, Spinner } from "../../components/Feedback";
import { chapterStream, type ChapterSession } from "../../domain/online";
import { chapterAt, webtoonHint } from "../../domain/reader";
import { ReadingLifecycle, serialQueue } from "../../domain/reading-lifecycle";
import { ApiError, api, errorMessage } from "../../services/api";
import { cancellable } from "../../services/events";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onlinePageUrl } from "../../services/urls";
import { SourceTrouble } from "../online/parts";
import { useLeaveFullscreenOnClose } from "./fullscreen";
import { APPEAR_LATE } from "./PageView";
import { Reader, type ReaderContent } from "./ReaderScreen";
import { sound } from "../../services/sound";

let requestCounter = 0;

/** Quantas páginas antes do fim o próximo capítulo já é pedido, para a passagem não ter espera. */
const NEAR_THE_END = 5;

/** O leitor avisa que continua aberto: sem isso, a sessão deixa de valer como "Lendo" (reading.rs, SESSION_STALE_MS). */
const HEARTBEAT_MS = 60_000;

/**
 * Liga as regras do ciclo de leitura ao backend. As gravações saem na ordem em que aconteceram, e a sessão
 * termina ao sair do leitor, ao trocar de capítulo e ao fechar a janela. Uma queda é tratada na próxima abertura.
 */
function useReadingLifecycle(): ReadingLifecycle {
  const [{ lifecycle, queue }] = useState(() => {
    const queue = serialQueue();
    const lifecycle = new ReadingLifecycle({
      acknowledge: (id) => void queue.push(() => api.chapterAcknowledge(id)),
      start: (id) => void queue.push(() => api.chapterReadingStart(id)),
      end: (id) => void queue.push(() => api.chapterReadingEnd(id)),
      complete: (id) => {
        void queue.push(() => api.chapterReadingComplete(id));
        sound.play("complete", { reader: true });
      },
      // Concluir e passar ao próximo acontecem juntos ao rolar: o serviço de som deixa tocar só o primeiro.
      enter: () => sound.play("chapter", { reader: true }),
    });
    return { lifecycle, queue };
  });
  useEffect(() => {
    const beat = window.setInterval(() => {
      const id = lifecycle.active;
      if (id !== null) void queue.push(() => api.chapterReadingTouch(id));
    }, HEARTBEAT_MS);
    const onUnload = () => lifecycle.close();
    window.addEventListener("beforeunload", onUnload);
    const offClose = cancellable(() =>
      getCurrentWindow().onCloseRequested(async () => {
        lifecycle.close();
        // Nunca impedir o fechamento por muito tempo: o que não der tempo, a recuperação resolve.
        await Promise.race([queue.idle, new Promise((done) => window.setTimeout(done, 800))]);
      }),
    );
    return () => {
      window.clearInterval(beat);
      window.removeEventListener("beforeunload", onUnload);
      offClose();
      lifecycle.close();
    };
  }, [lifecycle, queue]);
  return lifecycle;
}

const isCancelled = (e: unknown) => e instanceof ApiError && e.code === "extension_cancelled";

/** Lê um capítulo de uma fonte sem baixar. Voltar leva à página da obra. */
export function ChapterReaderScreen({ id }: { id: number }) {
  const nav = useNav();
  const [chapterId, setChapterId] = useState(id);
  const [attempt, setAttempt] = useState(0);
  const [result, setResult] = useState<{ key: string; session?: ChapterSession; error?: string; protected?: boolean } | null>(null);
  const zoomState = useState(1);
  useLeaveFullscreenOnClose();

  const key = `${chapterId}:${attempt}`;
  useEffect(() => {
    let alive = true;
    const requestId = `chapter-${++requestCounter}`;
    api
      .chapterOpen(requestId, chapterId)
      .then((session) => alive && setResult({ key, session }))
      .catch((e: unknown) => {
        if (alive && !isCancelled(e)) setResult({ key, error: errorMessage(e), protected: e instanceof ApiError && e.code === "source_protected" });
      });
    return () => {
      alive = false;
      void api.sourceCancel(requestId).catch(() => undefined);
    };
  }, [chapterId, key]);

  const current = result?.key === key ? result : null;
  // Da tela inicial ou da biblioteca, voltar não leva à página da obra.
  const fromChapters = nav.stack.at(-2)?.name === "series";
  const backText = fromChapters ? "Voltar aos capítulos" : "Voltar";
  const retry = () => setAttempt((n) => n + 1);
  const openChapter = useCallback((next: number) => {
    sound.play("chapter", { reader: true });
    setChapterId(next);
  }, []);

  if (current?.error) {
    return (
      <div className="flex h-full flex-col bg-bg">
        <div className="flex items-center gap-2 border-b border-border px-4 py-2">
          <Button variant="ghost" icon={ArrowLeft} onClick={nav.back}>
            {backText}
          </Button>
        </div>
        <div className="flex flex-1 items-center justify-center p-6">
          <SourceTrouble message={current.error} site={current.protected ? { chapterId } : undefined} onRetry={retry} center>
            {(text, actions) => <ErrorState title="Não foi possível abrir este capítulo" message={text} action={actions} />}
          </SourceTrouble>
        </div>
      </div>
    );
  }
  if (!current?.session) {
    return (
      <div className="flex h-full items-center justify-center bg-reader text-fg">
        <div className="flex flex-col items-center gap-3" style={APPEAR_LATE}>
          <Spinner size={22} label="Abrindo capítulo…" />
          {/* A fonte pode demorar: ninguém fica preso esperando. */}
          <Button variant="ghost" size="sm" onClick={nav.back}>
            {backText}
          </Button>
        </div>
      </div>
    );
  }
  return <ChapterStream key={key} first={current.session} zoomState={zoomState} onOpen={openChapter} fromChapters={fromChapters} />;
}

/** Os capítulos em sequência, como uma leitura só: o próximo entra no fim, sem trocar de tela. */
function ChapterStream({ first, zoomState, onOpen, fromChapters }: { first: ChapterSession; zoomState: [number, Dispatch<SetStateAction<number>>]; onOpen: (id: number) => void; fromChapters: boolean }) {
  const [sessions, setSessions] = useState([first]);
  const [failure, setFailure] = useState<{ message: string; protected: boolean } | null>(null);
  const request = useRef<string | null>(null);
  const lifecycle = useReadingLifecycle();

  const nextId = sessions.at(-1)!.nextId;
  const chapters = useMemo(() => chapterStream(sessions), [sessions]);
  lifecycle.setChapters(chapters);
  const last = chapters.at(-1)!;
  const total = last.first + last.count;

  const loadNext = useCallback(() => {
    if (nextId === null || request.current) return;
    const requestId = `chapter-${++requestCounter}`;
    request.current = requestId;
    setFailure(null);
    api
      .chapterOpen(requestId, nextId)
      .then((session) => setSessions((list) => [...list, session]))
      .catch((e: unknown) => {
        if (!isCancelled(e)) setFailure({ message: errorMessage(e), protected: e instanceof ApiError && e.code === "source_protected" });
      })
      .finally(() => {
        request.current = null;
      });
  }, [nextId]);

  useEffect(
    () => () => {
      if (request.current) void api.sourceCancel(request.current).catch(() => undefined);
    },
    [],
  );

  const onPage = useCallback(
    (page: number) => {
      if (failure === null && page >= total - NEAR_THE_END) loadNext();
    },
    [total, failure, loadNext],
  );

  const content = useMemo<ReaderContent>(() => {
    const { previousId } = first;
    return {
      title: first.seriesTitle,
      pageCount: total,
      chapters,
      start: first.start,
      pageSrc: (index, token) => {
        const chapter = chapterAt(chapters, index);
        return onlinePageUrl(chapter.id, index - chapter.first, token);
      },
      saveProgress: (page, _count, ratio) => {
        const chapter = chapterAt(chapters, page);
        return api.chapterSaveProgress(chapter.id, page - chapter.first, chapter.count, ratio);
      },
      resetProgress: (page) => api.chapterResetProgress(chapterAt(chapters, page).id),
      backLabel: fromChapters ? "Capítulos" : "Voltar",
      backTitle: fromChapters ? "Voltar aos capítulos" : "Voltar",
      chosenMode: first.readerMode,
      webtoonHint: webtoonHint(first.genres, first.pageCount),
      saveMode: (mode) => void api.seriesSetReaderMode(first.seriesId, mode).catch(() => undefined),
      onPage,
      waitingForNext: nextId !== null,
      ending: (
        <div className="flex flex-col items-center gap-2 text-center text-sm text-muted">
          {failure !== null ? (
            <SourceTrouble message={failure.message} site={failure.protected && nextId !== null ? { chapterId: nextId } : undefined} onRetry={loadNext} size="sm" center>
              {(text, actions) => (
                <>
                  <p className="font-medium text-fg">Não foi possível abrir o próximo capítulo</p>
                  <p className="max-w-md">{text}</p>
                  {actions}
                </>
              )}
            </SourceTrouble>
          ) : nextId !== null ? (
            <Spinner size={16} label="Abrindo o próximo capítulo…" />
          ) : (
            <p>Não há mais capítulos</p>
          )}
        </div>
      ),
      previousChapter: previousId === null ? undefined : () => onOpen(previousId),
      nextChapter: nextId === null ? undefined : () => onOpen(nextId),
      lifecycle,
    };
  }, [first, chapters, total, nextId, failure, onPage, loadNext, onOpen, fromChapters, lifecycle]);

  return <Reader content={content} pdf={null} zoomState={zoomState} />;
}
