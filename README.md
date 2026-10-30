# Cyder

Um cliente desktop para o Spotify, escrito em Rust, com visual inspirado no [Cider](https://cider.sh/) e uso de memória bem menor que o app oficial.

O Cyder toca a sua biblioteca do Spotify direto no computador: curtidas, álbuns, playlists, podcasts e busca. Tem letra sincronizada, modo imersivo e mini player.

> O Cyder não é afiliado ao Spotify. É um projeto pessoal e de código aberto.

## Por que existe

O app oficial do Spotify para desktop é um Chromium embutido (CEF). Na mesma máquina, ele ocupou **~765 MB** de memória em 8 processos. O Cyder é um processo só, sem navegador por baixo:

| App | Memória (Windows, memória privada) |
|-----|------------------------------------|
| Spotify oficial | ~765 MB |
| Cyder tocando, com a biblioteca aberta | 80 a 123 MB |

São mais ou menos **6 vezes menos**. Como os números foram medidos, e o orçamento de memória de cada tela, está em `docs/03-arquitetura.md` e `docs/07-resultados-spikes.md`.

## O que ele faz

**Ouvir**
- Toca a música no próprio computador (via [librespot](https://github.com/librespot-org/librespot)), com qualidade até 320 kbps.
- Aparece como aparelho no Spotify Connect: dá para controlar pelo celular, e o Cyder também controla outros aparelhos.
- Fila que dá para reordenar arrastando, aleatório, repetir e um "aleatório inteligente" que mistura recomendações na fila.
- Equalizador de 6 faixas e transição suave entre músicas.
- Abre onde parou: mesma fila, faixa e posição.

**Ver**
- Letra sincronizada pelo [LRCLIB](https://lrclib.net/), rolando com a música; clique numa linha para pular até ela.
- Modo imersivo: capa grande, fundo borrado com a cor da capa e, quando a música tem, o Canvas (o vídeo curto) no fundo.
- Mini player sempre por cima das outras janelas.
- Tema escuro, claro ou o do sistema.

**Biblioteca**
- Curtidas, álbuns, artistas, playlists e podcasts, com listas que rolam leves mesmo com milhares de faixas.
- Busca enquanto você digita (Ctrl+K).
- Criar, renomear, trocar a capa e reordenar as suas playlists.
- Novidades dos artistas que você segue, artistas parecidos e créditos das músicas.
- Estatísticas do que você ouviu neste computador.

**Integrações (opcionais)**
- Last.fm: "ouvindo agora" e scrobbles.
- Discord: mostra no seu perfil o que está tocando.
- Ouvir junto: outra pessoa segue o que você toca, cada uma pela própria conta.

**No Windows**
- Ícone na bandeja, abrir com o Windows e atalhos de teclado.
- Atualização automática pelos Releases do GitHub: o app avisa, baixa, confere o SHA-256 e reinicia.

## Antes de instalar

- **Spotify Premium.** O Spotify só deixa clientes como este tocarem música em contas Premium.
- **Um app seu no painel do Spotify.** Cada pessoa usa o próprio Client ID, criado de graça em [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard). A primeira abertura do Cyder mostra o passo a passo e leva uns cinco minutos.
- **Windows 10 ou 11.** O código é multiplataforma, mas macOS e Linux ainda não foram testados.

## Instalar

1. Baixe o `cyder.exe` mais recente em [Releases](https://github.com/GabrielSantos23/Cyder/releases).
2. Abra. Na primeira vez, o Cyder pede o Client ID:
   1. No painel do Spotify, clique em **Create app**.
   2. Em **Redirect URIs**, adicione `http://127.0.0.1:5589/callback`.
   3. Marque **Web API** e salve.
   4. Copie o **Client ID** (em Settings › Basic Information) e cole no Cyder.
3. O navegador abre para você entrar na conta do Spotify. A senha fica no site do Spotify; o Cyder não a vê.

O login fica guardado no Gerenciador de Credenciais do Windows, e as configurações em `%APPDATA%\cyder`.

## Compilar

Precisa do Rust (a versão fica fixada em `rust-toolchain.toml`).

```bash
cargo build --release -p cyder
```

O executável sai em `target/release/cyder.exe`. Para ver o registro detalhado:

```bash
RUST_LOG=cyder=debug,cyder_core=debug target/release/cyder.exe
```

### Publicar uma versão

1. Mude `version` em `[workspace.package]` no `Cargo.toml`.
2. Faça o commit e crie a tag com a mesma versão (`git tag v0.2.0`, depois `git push origin v0.2.0`).
3. A Action `.github/workflows/release.yml` compila no Windows, confere se a tag bate com o `Cargo.toml` e publica o `cyder.exe` com o `.sha256`.

## Como é feito

| Parte | O que usa |
|-------|-----------|
| Interface | [GPUI](https://github.com/sonorahq/gpui) (fork da sonorahq, com desfoque de fundo), a mesma base do editor Zed |
| Áudio e Spotify Connect | librespot 0.8 |
| Biblioteca, busca, playlists | Web API do Spotify, com o seu Client ID |
| Letras | LRCLIB, com cache local |

O código fica em dois crates:
- `crates/core`: áudio, API, cache, letras e cores, sem depender da interface.
- `crates/app`: as telas.

A pasta `spikes/` guarda os protótipos usados para escolher a stack. Um deles é a versão em Tauri, que ficou em 250 a 275 MB.

## Limitações

- **Não conta para o Wrapped.** O que toca pelo Cyder não entra no histórico do Spotify nem nas recomendações. É uma limitação do librespot, sem contorno conhecido. O Last.fm registra normalmente.
- **Sem modo offline.** O Spotify não permite baixar músicas fora do app oficial. O Cyder guarda um cache de áudio para não baixar a mesma música de novo, mas ele não serve para ouvir sem internet.
- **Funções exclusivas do Spotify ficam de fora:** DJ, Jam, Blend, AI Playlist e podcasts em vídeo não têm API.
- **Crossfade de verdade, não.** O player toca uma música por vez; a transição suave é um fade no fim de uma e no começo da outra.
- **Limite de pedidos.** O Spotify limita quantos pedidos cada Client ID faz. O Cyder guarda quase tudo em cache para pedir pouco, mas se o limite estourar, a biblioteca para de atualizar até o prazo que o Spotify der.
- **Apps em modo de desenvolvimento** aceitam até 5 usuários. Por isso cada pessoa cria o próprio app.
- **Só testado no Windows.**
- **Política do Spotify.** Os termos para desenvolvedores proíbem produtos que substituem a experiência principal do Spotify sem permissão. O Cyder é um projeto pessoal, sem fins comerciais, e não é publicado em lojas.
