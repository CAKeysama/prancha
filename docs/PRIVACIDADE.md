# Aviso de privacidade do Prancha

O Prancha funciona no seu computador e não tem conta. Suas HQs, o progresso, os favoritos, as coleções e as obras que você acompanha ficam só na pasta de dados do aplicativo.

## Quando o Prancha usa a internet

- **Atualizações:** para saber se há uma versão nova, o Prancha consulta as releases do projeto no GitHub.
- **Fontes de leitura (plugin opcional):** quando você usa repositórios e extensões, o Prancha fala com os sites dessas fontes para buscar obras, capas e páginas.
- **Telemetria (opcional, desligada até você aceitar):** estatísticas de uso enviadas ao servidor do projeto. Detalhes abaixo.
- **MyAnimeList (opcional, só com a conta conectada):** o Prancha procura no catálogo do MAL o título, o autor e o número de capítulos das obras das fontes que você abre, e atualiza os capítulos lidos na sua lista quando você termina um capítulo. Você entra pelo site do MAL, e o Prancha não vê sua senha; o acesso fica cifrado pelo Windows neste computador. Desconectar apaga esse acesso. Detalhes em [MYANIMELIST.md](MYANIMELIST.md).

## Telemetria

**Para que serve.** Saber quais versões estão em uso, quais fontes são mais usadas ou falham e onde o Prancha trava ou fica lento, para decidir o que corrigir primeiro.

**É opcional.** Fica desligada até você aceitar, no convite que aparece uma vez depois do tutorial ou em **Configurações › Privacidade**. O Prancha funciona igual sem ela. Você pode desligar quando quiser, e ela nunca volta a ligar sozinha.

### O que é enviado

- Versão do Prancha, sistema (Windows, macOS ou Linux), arquitetura e tipo de instalador.
- Quantas vezes o app abriu por dia e se a vez anterior fechou de forma inesperada.
- Por fonte de leitura, só o identificador técnico da extensão e da fonte: quantas buscas, obras e capítulos foram abertos e quantos erros houve.
- Falhas por categoria, componente e código técnico (por exemplo, "network / sources / timeout"), somadas por hora.
- Tempos de abertura do app, das listas de Explorar, de obras e de capítulos.
- Um identificador aleatório desta instalação, criado quando você aceita, que serve só para contar instalações.

Tudo é somado por dia no seu computador antes de sair.

### O que nunca é enviado

- Títulos, capas, buscas, páginas ou qualquer conteúdo que você lê ou importa.
- Histórico de leitura, progresso, favoritos ou coleções.
- Nomes de arquivos e pastas, caminhos do computador, mensagens de erro em texto.
- Nome, e-mail, contas, senhas, tokens, cookies ou endereços de sites.
- Endereço IP: o servidor não o grava junto com os dados.

### Anônimo até onde?

O identificador é aleatório e não tem relação com você, com sua conta em qualquer serviço ou com o seu computador. Mas ele é o mesmo em todos os envios desta instalação até você trocá-lo ou desligar a telemetria. Por isso, os dados são **pseudônimos**, não anônimos no sentido absoluto: dá para saber que uma mesma instalação abriu o app em dias diferentes, sem saber de quem é.

Como qualquer conexão com a internet, o servidor vê o endereço IP de quem envia. Ele não é gravado com os dados de telemetria, mas o provedor de hospedagem (Vercel) mantém registros de acesso por um tempo limitado.

### Onde ficam e por quanto tempo

Os dados vão para uma função na Vercel, que confere cada envio, e são guardados num banco Supabase do projeto, em São Paulo. Ficam 180 dias e depois são apagados automaticamente. Só os administradores do Prancha veem os totais, num painel protegido por login que não mostra instalações individuais. O aplicativo só envia: nem ele consegue ler de volta o que foi guardado. O acesso direto ao banco fica restrito a quem administra o projeto.

### Seus controles

Em **Configurações › Privacidade**:

- **Desligar:** para os envios na hora, descarta o que ainda não tinha saído e apaga o identificador do seu computador.
- **Gerar outro:** troca o identificador. O que foi enviado antes deixa de estar ligado ao que vier depois.
- **Apagar dados:** desliga a telemetria e pede ao servidor que apague tudo que esta instalação enviou. Se o servidor não responder, o pedido fica guardado para você tentar de novo.

Os detalhes técnicos estão em [TELEMETRY.md](TELEMETRY.md). Dúvidas ou pedidos: abra uma [issue no GitHub](https://github.com/CAKeysama/prancha/issues).
