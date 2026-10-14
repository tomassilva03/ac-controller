# AC Controller

App de bandeja (Windows) e barra de menus (macOS) para controlar um ar condicionado Haier através da conta hOn.

Estado atual: a interface está feita e funciona com um AC simulado. A ligação real ao hOn vem a seguir.

## Desenvolver

Precisas de [Node 22](https://nodejs.org) e [Rust](https://rustup.rs), mais os [pré-requisitos do Tauri](https://v2.tauri.app/start/prerequisites/) do teu sistema.

```
npm install
npm run dev
```

O ícone aparece na bandeja (Windows) ou na barra de menus (macOS). Clique esquerdo abre o painel, clique direito mostra "Sair".

Para ver só o design, também podes abrir `src/index.html` no browser.

## Instaladores

Cada push para `main` gera um `.msi` (Windows) e um `.dmg` (macOS) no separador **Actions** do GitHub, em *Artifacts*.

## Estrutura

- `src/` interface (HTML, CSS e JS, sem framework)
- `src-tauri/src/lib.rs` ícone da bandeja, janela e comandos
- `src-tauri/src/ac.rs` estado do AC (simulado por agora)
