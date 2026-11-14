// Estados da Leitura Guiada difíceis de provocar no aplicativo real: falha da análise e região duvidosa.
import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { ToastProvider } from "../../src/components/Toast";
import type { GuidedRegion } from "../../src/domain/guided";
import { DEFAULT_SHORTCUTS } from "../../src/domain/shortcuts";
import { GuidedReader } from "../../src/features/reader/GuidedReader";

const region = (id: string, x: number, confidence: number): GuidedRegion => ({
  id,
  pageIndex: 0,
  x,
  y: 0.1,
  w: 0.2,
  h: 0.1,
  kind: "speech",
  order: 0,
  confidence,
  review: "auto",
  outline: [],
});

function open(guidedPage: () => unknown, onExit = vi.fn()) {
  mockIPC((cmd) => (cmd === "guided_page" ? guidedPage() : null));
  render(
    <ToastProvider>
      <GuidedReader comicId={1} page={0} pageCount={3} pdf={null} rtl={false} shortcuts={DEFAULT_SHORTCUTS} natural={{ width: 1000, height: 1500 }} onSize={() => undefined} onPage={() => undefined} onExit={onExit} />
    </ToastProvider>,
  );
  return onExit;
}

afterEach(() => clearMocks());

describe("Leitura Guiada: estados", () => {
  it("falha na análise não trava o leitor e oferece outro caminho", async () => {
    let attempts = 0;
    const user = userEvent.setup();
    open(() => {
      attempts += 1;
      if (attempts === 1) throw { code: "undecodable", message: "Não foi possível analisar esta página (formato)." };
      return { pageIndex: 0, source: "auto", regions: [region("a", 0.1, 0.9)], panels: [], needsReview: false };
    });
    expect(await screen.findByText("Não foi possível analisar esta página")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Marcar manualmente" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Tentar de novo" }));
    expect(await screen.findByTestId("guided-progress")).toHaveTextContent("1 de 1");
  });

  it("região duvidosa aparece marcada e pede revisão", async () => {
    open(() => ({ pageIndex: 0, source: "auto", regions: [region("a", 0.1, 0.4)], panels: [], needsReview: true }));
    expect(await screen.findByRole("button", { name: "Corrigir regiões (há regiões duvidosas)" })).toBeInTheDocument();
    expect(await screen.findByTestId("guided-spot-doubtful")).toBeInTheDocument();
  });

  it("o balão cresce até o tamanho final sem passar dele", async () => {
    open(() => ({ pageIndex: 0, source: "auto", regions: [region("a", 0.1, 0.9)], panels: [], needsReview: false }));
    const spot = await screen.findByTestId("guided-spot");
    expect(spot.style.transition).toContain("cubic-bezier(0.16, 1, 0.3, 1)");
  });

  it("Esc sai enquanto a análise ainda está em andamento", async () => {
    const user = userEvent.setup();
    const onExit = open(() => new Promise(() => undefined));
    expect(await screen.findByText("Analisando página…")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await vi.waitFor(() => expect(onExit).toHaveBeenCalled());
  });
});
