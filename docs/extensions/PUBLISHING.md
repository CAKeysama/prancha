# Publicar um repositório

Um repositório é uma pasta de arquivos estáticos servida em HTTPS na porta 443. O `prancha-ext` gera e assina tudo. Nos exemplos, `prancha-ext` é o binário em `extensions/target/release/`, compilado como em [Desenvolver uma extensão](DEVELOPMENT.md).

## 1. Criar a chave

```bash
prancha-ext keygen minha-chave.key
```

O comando mostra a chave pública e a impressão digital, e não sobrescreve um arquivo que já exista.

**A chave privada nunca vai para um repositório, um log ou uma mensagem.** Guarde fora do projeto e faça uma cópia. Quem tiver a chave publica em seu nome. Quem a perder precisa trocar de chave, e cada pessoa que usa o repositório recebe um aviso e precisa confirmar de novo.

## 2. Descrever o repositório

`repositorio.json`:

```json
{
  "id": "exemplo.repositorio",
  "name": "Repositório de exemplo",
  "baseUrl": "https://exemplo.com/repo",
  "description": "O que ele oferece.",
  "website": "https://exemplo.com"
}
```

`baseUrl` é onde os arquivos ficarão publicados.

## 3. Descrever cada extensão

`minha-fonte.json`, ao lado do componente:

```json
{
  "artifact": "minha-fonte.wasm",
  "id": "exemplo.repositorio.fonte",
  "name": "Minha fonte",
  "version": "1.0.0",
  "description": "O que ela oferece.",
  "authors": ["Alguém"],
  "languages": ["pt-BR"],
  "minAppVersion": "0.2.0",
  "supportedKinds": ["comic"],
  "contentRating": "general",
  "sources": [{ "id": "exemplo", "name": "Fonte de exemplo", "language": "pt-BR", "contentRating": "general" }],
  "capabilities": { "network": true, "storage": false, "authentication": false, "allowedHosts": ["exemplo.com"] }
}
```

O significado dos campos está em [Formato dos repositórios](REPOSITORY_FORMAT.md). O `prancha-ext` acrescenta `runtime`, `apiVersion`, `artifactUrl`, `signatureUrl`, `size` e `sha256`.

As fontes e as classificações precisam ser as mesmas que o componente declara em `list-sources`. Se forem diferentes, a instalação é recusada.

## 4. Gerar

```bash
prancha-ext publish --key minha-chave.key --repo repositorio.json --out publicar minha-fonte.json
```

Liste todas as extensões do repositório: o índice é refeito a cada publicação. Para publicar uma versão nova, aumente `version` e gere de novo. Pacotes antigos que continuarem na pasta não fazem mal, mas não aparecem no catálogo.

## 5. Verificar

```bash
prancha-ext verify publicar
```

O comando confere a assinatura do índice e o SHA-256 e a assinatura de cada pacote, e mostra a impressão digital.

## 6. Publicar os arquivos

Envie a pasta inteira para o endereço de `baseUrl`. GitHub Pages, um bucket ou qualquer hospedagem estática servem. Depois, divulgue o endereço do `prancha-repo.json` e a impressão digital **em um canal separado**, para que as pessoas possam comparar.

Publique primeiro os pacotes e por último o índice, para que ninguém leia um índice que aponta para um pacote que ainda não existe.

## Exemplo completo

[`scripts/build-demo-repo.mjs`](../../scripts/build-demo-repo.mjs) faz todos os passos para o repositório de demonstração, inclusive desenhar as imagens:

```bash
node scripts/build-demo-repo.mjs --out publicar --base-url https://exemplo.com/repo --key minha-chave.key
```

## Trocar de chave

Publique com a chave nova. Cada pessoa verá um aviso em Repositórios e precisará revisar a impressão digital e aceitar. Até lá, o catálogo anterior continua na tela e nada é atualizado. Não faça isso sem avisar antes pelo seu canal: uma troca inesperada é exatamente o que um ataque pareceria.

## Regras de conteúdo

Um repositório não pode oferecer conteúdo adulto: essas entradas são descartadas. Só publique o que você tem o direito de publicar, e não inclua uma fonte que contorne login, paywall, DRM ou CAPTCHA.
