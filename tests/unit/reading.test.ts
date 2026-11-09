// Ciclo de leitura de um capítulo (src/domain/reading-lifecycle.ts): regras puras, sem React nem backend.
import { describe, expect, it } from "vitest";
import { ReadingLifecycle, serialQueue, type LifecycleChapter } from "../../src/domain/reading-lifecycle";

// Capítulo A: páginas 0 a 4. Capítulo B: páginas 5 a 7.
const A: LifecycleChapter = { id: 1, first: 0, count: 5 };
const B: LifecycleChapter = { id: 2, first: 5, count: 3 };

function setup(chapters: LifecycleChapter[] = [A]) {
  const events: string[] = [];
  const lifecycle = new ReadingLifecycle({
    acknowledge: (id) => events.push(`acknowledge:${id}`),
    start: (id) => events.push(`start:${id}`),
    end: (id) => events.push(`end:${id}`),
    complete: (id) => events.push(`complete:${id}`),
  });
  lifecycle.setChapters(chapters);
  const count = (event: string) => events.filter((e) => e === event).length;
  return { lifecycle, events, count };
}

describe("ReadingLifecycle", () => {
  it("restaurar a posição ao abrir não começa a leitura", () => {
    const { lifecycle, events } = setup();
    lifecycle.view({ pages: [2], how: "restore" });
    lifecycle.view({ pages: [2], how: "restore" });
    expect(events).toEqual(["acknowledge:1"]);
    expect(lifecycle.active).toBeNull();
  });

  it("o primeiro passo começa a leitura uma vez só, mesmo com a tela repetida", () => {
    const { lifecycle, events, count } = setup();
    lifecycle.view({ pages: [0], how: "restore" });
    lifecycle.view({ pages: [1], how: "step" });
    lifecycle.view({ pages: [1], how: "step" });
    lifecycle.view({ pages: [2], how: "step" });
    lifecycle.view({ pages: [1], how: "step" });
    expect(count("start:1")).toBe(1);
    expect(events).toEqual(["acknowledge:1", "start:1"]);
  });

  it("um salto começa a leitura, mas não conclui nem na última página", () => {
    const { lifecycle, events } = setup();
    lifecycle.view({ pages: [0], how: "restore" });
    lifecycle.view({ pages: [4], how: "jump" });
    lifecycle.pageRendered(4);
    expect(events).toEqual(["acknowledge:1", "start:1"]);
    expect(lifecycle.active).toBe(1);
  });

  it("um passo até a última página conclui só depois de ela ser desenhada", () => {
    const { lifecycle, events } = setup();
    lifecycle.view({ pages: [3], how: "step" });
    lifecycle.view({ pages: [4], how: "step" });
    expect(events).not.toContain("complete:1");
    lifecycle.pageRendered(4);
    expect(events).toEqual(["acknowledge:1", "start:1", "complete:1"]);
  });

  it("se a última página já estava desenhada, chegar nela por passo conclui na hora", () => {
    const { lifecycle, events } = setup();
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [3], how: "step" });
    expect(events).not.toContain("complete:1");
    lifecycle.view({ pages: [4], how: "step" });
    expect(events).toEqual(["acknowledge:1", "start:1", "complete:1"]);
  });

  it("na página dupla, a última página na tela conta", () => {
    const { lifecycle, events } = setup();
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [1, 2], how: "step" });
    lifecycle.view({ pages: [3, 4], how: "step" });
    expect(events).toEqual(["acknowledge:1", "start:1", "complete:1"]);
  });

  it("desenhar de novo ou repetir a tela não conclui duas vezes", () => {
    const { lifecycle, count } = setup();
    lifecycle.view({ pages: [3], how: "step" });
    lifecycle.view({ pages: [4], how: "step" });
    lifecycle.pageRendered(4);
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [4], how: "step" });
    lifecycle.view({ pages: [3], how: "step" });
    lifecycle.view({ pages: [4], how: "step" });
    lifecycle.pressedNextAtEnd();
    expect(count("complete:1")).toBe(1);
    // Depois de concluir, voltar páginas não abre outra sessão.
    expect(count("start:1")).toBe(1);
  });

  it("rolagem vertical: só conclui quando a borda de baixo da última página passa", () => {
    const { lifecycle, events, count } = setup();
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [3], how: "scroll", passed: 2 });
    lifecycle.view({ pages: [4], how: "scroll", passed: 3 });
    expect(events).toEqual(["acknowledge:1", "start:1"]);
    lifecycle.view({ pages: [4], how: "scroll", passed: 4 });
    lifecycle.view({ pages: [4], how: "scroll", passed: 4 });
    expect(count("complete:1")).toBe(1);
  });

  it("rolagem vertical: passar da última página sem ela desenhada não conclui", () => {
    const { lifecycle, events } = setup();
    lifecycle.view({ pages: [4], how: "scroll", passed: 4 });
    expect(events).not.toContain("complete:1");
    lifecycle.pageRendered(4);
    expect(events).toContain("complete:1");
  });

  it("restaurar a posição no fim do capítulo não conclui", () => {
    const { lifecycle, events } = setup();
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [4], how: "restore", passed: 4 });
    lifecycle.pageRendered(3);
    expect(events).toEqual(["acknowledge:1"]);
    expect(lifecycle.active).toBeNull();
  });

  it("passar de A para B encerra A sem concluir e reconhece B uma vez", () => {
    const { lifecycle, events, count } = setup([A, B]);
    lifecycle.view({ pages: [1], how: "step" });
    lifecycle.view({ pages: [5], how: "step" });
    lifecycle.view({ pages: [6], how: "step" });
    lifecycle.view({ pages: [6], how: "restore" });
    expect(events).toEqual(["acknowledge:1", "start:1", "end:1", "acknowledge:2", "start:2"]);
    expect(count("acknowledge:2")).toBe(1);
    expect(lifecycle.active).toBe(2);
  });

  it("sair de A só por restauração não encerra nada, porque A nem começou", () => {
    const { lifecycle, events } = setup([A, B]);
    lifecycle.view({ pages: [2], how: "restore" });
    lifecycle.view({ pages: [5], how: "restore" });
    expect(events).toEqual(["acknowledge:1", "acknowledge:2"]);
  });

  it("rolar além do fim de A para dentro de B conclui A", () => {
    const { lifecycle, events, count } = setup([A, B]);
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [3], how: "scroll", passed: 2 });
    lifecycle.view({ pages: [4], how: "scroll", passed: 4 });
    lifecycle.view({ pages: [5], how: "scroll", passed: 4 });
    lifecycle.view({ pages: [6], how: "scroll", passed: 5 });
    expect(events).toEqual(["acknowledge:1", "start:1", "complete:1", "acknowledge:2", "start:2"]);
    expect(count("complete:1")).toBe(1);
    expect(count("end:1")).toBe(0);
  });

  it("uma rolagem rápida que já cai em B conclui A uma vez", () => {
    const { lifecycle, count } = setup([A, B]);
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [3], how: "scroll", passed: 2 });
    lifecycle.view({ pages: [5], how: "scroll", passed: 4 });
    expect(count("complete:1")).toBe(1);
    expect(count("complete:2")).toBe(0);
    expect(count("start:2")).toBe(1);
  });

  it("fechar encerra o capítulo aberto uma vez, e repetir não faz nada", () => {
    const { lifecycle, events, count } = setup();
    lifecycle.view({ pages: [1], how: "step" });
    lifecycle.close();
    lifecycle.close();
    expect(count("end:1")).toBe(1);
    expect(events).toEqual(["acknowledge:1", "start:1", "end:1"]);
    expect(lifecycle.active).toBeNull();
  });

  it("fechar depois de concluir ou sem leitura efetiva não encerra", () => {
    const done = setup();
    done.lifecycle.pageRendered(4);
    done.lifecycle.view({ pages: [3], how: "step" });
    done.lifecycle.view({ pages: [4], how: "step" });
    done.lifecycle.close();
    expect(done.count("end:1")).toBe(0);

    const idle = setup();
    idle.lifecycle.view({ pages: [2], how: "restore" });
    idle.lifecycle.close();
    expect(idle.events).toEqual(["acknowledge:1"]);
  });

  it("depois de fechar, a próxima interação efetiva abre uma sessão nova", () => {
    const { lifecycle, count } = setup();
    lifecycle.view({ pages: [1], how: "step" });
    lifecycle.close();
    lifecycle.view({ pages: [1], how: "restore" });
    expect(count("start:1")).toBe(1);
    lifecycle.view({ pages: [2], how: "step" });
    expect(count("start:1")).toBe(2);
    // O reconhecimento continua sendo um só por capítulo.
    expect(count("acknowledge:1")).toBe(1);
  });

  it("reconhece cada capítulo uma vez, na primeira vez que aparece, mesmo restaurado", () => {
    const { lifecycle, count } = setup([A, B]);
    lifecycle.view({ pages: [0], how: "restore" });
    expect(count("acknowledge:1")).toBe(1);
    lifecycle.view({ pages: [1], how: "step" });
    lifecycle.view({ pages: [5], how: "jump" });
    lifecycle.view({ pages: [2], how: "jump" });
    lifecycle.view({ pages: [6], how: "step" });
    expect(count("acknowledge:1")).toBe(1);
    expect(count("acknowledge:2")).toBe(1);
  });

  it("Próxima na última página depois de um salto conclui, assim que a página é desenhada", () => {
    const { lifecycle, events, count } = setup();
    lifecycle.view({ pages: [4], how: "jump" });
    lifecycle.pressedNextAtEnd();
    expect(events).not.toContain("complete:1");
    lifecycle.pageRendered(4);
    expect(events).toEqual(["acknowledge:1", "start:1", "complete:1"]);
    lifecycle.pressedNextAtEnd();
    expect(count("complete:1")).toBe(1);
  });

  it("Próxima na última página restaurada começa e conclui", () => {
    const { lifecycle, events } = setup();
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [4], how: "restore" });
    lifecycle.pressedNextAtEnd();
    expect(events).toEqual(["acknowledge:1", "start:1", "complete:1"]);
  });

  it("Próxima antes de qualquer tela não faz nada", () => {
    const { lifecycle, events } = setup();
    lifecycle.pressedNextAtEnd();
    expect(events).toEqual([]);
  });

  it("active: só o capítulo com leitura efetiva e ainda não concluído", () => {
    const { lifecycle } = setup([A, B]);
    expect(lifecycle.active).toBeNull();
    lifecycle.view({ pages: [0], how: "restore" });
    expect(lifecycle.active).toBeNull();
    lifecycle.view({ pages: [3], how: "step" });
    expect(lifecycle.active).toBe(1);
    lifecycle.pageRendered(4);
    lifecycle.view({ pages: [4], how: "step" });
    expect(lifecycle.active).toBeNull();
    lifecycle.view({ pages: [5], how: "step" });
    expect(lifecycle.active).toBe(2);
    lifecycle.close();
    expect(lifecycle.active).toBeNull();
  });

  it("páginas fora dos capítulos ou tela vazia são ignoradas", () => {
    const { lifecycle, events } = setup([A]);
    lifecycle.view({ pages: [], how: "step" });
    lifecycle.view({ pages: [9], how: "step" });
    expect(events).toEqual([]);
  });
});

describe("serialQueue", () => {
  const later = (ms: number, run: () => void) => () => new Promise<void>((resolve) => setTimeout(() => (run(), resolve()), ms));

  it("executa na ordem pedida, mesmo quando a primeira tarefa é mais lenta", async () => {
    const queue = serialQueue();
    const order: number[] = [];
    void queue.push(later(15, () => order.push(1)));
    void queue.push(later(1, () => order.push(2)));
    void queue.push(later(5, () => order.push(3)));
    await queue.idle;
    expect(order).toEqual([1, 2, 3]);
  });

  it("uma falha não impede as seguintes", async () => {
    const queue = serialQueue();
    const order: string[] = [];
    void queue.push(later(1, () => order.push("a")));
    const failed = queue.push(() => Promise.reject(new Error("falha")));
    void queue.push(later(1, () => order.push("c")));
    await expect(failed).resolves.toBeUndefined();
    await queue.idle;
    expect(order).toEqual(["a", "c"]);
  });

  it("uma tarefa só começa depois que a anterior termina", async () => {
    const queue = serialQueue();
    let running = 0;
    let peak = 0;
    const task = () => async () => {
      running++;
      peak = Math.max(peak, running);
      await new Promise((resolve) => setTimeout(resolve, 2));
      running--;
    };
    for (let i = 0; i < 4; i++) void queue.push(task());
    await queue.idle;
    expect(peak).toBe(1);
  });
});
