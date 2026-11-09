# Como contribuir

## Toda mudança é uma versão nova

Cada PR que entra na `main` publica uma versão do Prancha, e o próprio PR traz a versão e as notas. O passo a passo completo está em [docs/RELEASING.md](docs/RELEASING.md).

1. Rotule cada commit e o título do PR como `tipo: descrição` ou `tipo(área): descrição`:
   - `feat:` recurso novo, sobe a **minor** (2.4.1 → 2.5.0);
   - `fix:`, `perf:`, `refactor:`, `style:`, `test:`, `docs:`, `build:`, `ci:`, `chore:`, `revert:`, `site:` sobem a **patch** (2.5.0 → 2.5.1);
   - `!` depois do tipo (`feat!:`) ou `BREAKING CHANGE: ...` no corpo, para o que quebra o que já existia, sobe a **major** (2.5.1 → 3.0.0).
2. Rode `npm run version:bump`, faça o commit `chore(release): versão X.Y.Z` e escreva as notas na entrada aberta no `CHANGELOG.md`.
3. Confira com `npm run version:check`.
4. Traga a `main` para o branch logo antes do merge. Se outro PR já usou a versão, rode `npm run version:bump` de novo.

A verificação **Versão** do PR fica vermelha se algum commit estiver sem rótulo ou se a versão não for a que os rótulos pedem.
