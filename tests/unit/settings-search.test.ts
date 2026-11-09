// Índice da pesquisa das configurações. A ligação com a tela (cada id existe na sua categoria) está em app.test.tsx.
import { describe, expect, it } from "vitest";
import { SETTINGS_INDEX, normalize, searchSettings, singular } from "../../src/domain/settings-search";

const ids = (query: string) => searchSettings(query).map((entry) => entry.id);

describe("pesquisa das configurações", () => {
  it("encontra os exemplos pedidos pela finalidade", () => {
    expect(ids("Tema")[0]).toBe("tema");
    expect(ids("Idioma")[0]).toBe("idioma");
    expect(ids("Atualização")[0]).toBe("procurar");
    expect(ids("Atualização")).toEqual(expect.arrayContaining(["versao", "procurar", "historico"]));
    const reader = SETTINGS_INDEX.filter((entry) => entry.place === "leitura").map((entry) => entry.id);
    expect(ids("Leitura")).toEqual(expect.arrayContaining(reader));
  });

  it("ignora maiúsculas, acentos e o plural", () => {
    expect(normalize("Ação Rápida")).toBe("acao rapida");
    expect(singular("atualizacoes")).toBe("atualizacao");
    expect(singular("temas")).toBe("tema");
    expect(ids("ATUALIZACOES")).toEqual(ids("atualização"));
    expect(ids("temas")).toEqual(["tema"]);
    expect(ids("atalho")).toEqual(ids("Atalhos"));
  });

  it("acha pelas palavras-chave, não só pelo título que aparece na tela", () => {
    expect(ids("modo escuro")).toEqual(["tema"]);
    expect(ids("dark")).toEqual(["tema"]);
    expect(ids("liberar espaço").sort()).toEqual(["limpar-miniaturas", "limpar-paginas"]);
  });

  it("exige todas as palavras, cada uma no começo de uma palavra da opção", () => {
    expect(ids("tema atalho")).toEqual([]);
    expect(ids("ema")).toEqual([]);
    expect(ids("   ")).toEqual([]);
    expect(ids("de")).toEqual([]);
    expect(ids("direção de leitura")).toEqual(["direcao"]);
  });

  it("cada opção tem um id único", () => {
    expect(new Set(SETTINGS_INDEX.map((entry) => entry.id)).size).toBe(SETTINGS_INDEX.length);
  });
});
