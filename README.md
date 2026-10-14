# AC Controller

App de bandeja (Windows) e barra de menus (macOS) para controlar um ar condicionado Haier através da conta hOn.

A ligação ao hOn é feita por `bridge/hon_bridge.py` (usa a biblioteca [pyhOn](https://github.com/Andre0512/pyhOn)), compilado num executável que a app arranca em segundo plano.

## Desenvolver

Precisas de [Node 22](https://nodejs.org) e [Rust](https://rustup.rs), mais os [pré-requisitos do Tauri](https://v2.tauri.app/start/prerequisites/) do teu sistema.

```
python -m pip install -r bridge/requirements.txt
python bridge/build.py   # compila o hon-bridge para src-tauri/binaries/
npm install
npm run dev
```

O ícone aparece na bandeja (Windows) ou na barra de menus (macOS). Clique esquerdo abre o painel, clique direito mostra "Sair".

Para ver só o design, também podes abrir `src/index.html` no browser.

## Versões e atualizações

Cada push para `main` publica uma versão nova nas [Releases](../../releases) (`.msi` para Windows e `.dmg` para macOS). A app procura versões novas ao arrancar e de 6 em 6 horas, e instala-as sozinha.

As versões são assinadas com a chave dos secrets `TAURI_SIGNING_PRIVATE_KEY` e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; a chave pública está em `src-tauri/tauri.conf.json`. Sem essa chave a app instalada recusa as atualizações.

## Estrutura

- `src/` interface (HTML, CSS e JS, sem framework)
- `src-tauri/src/lib.rs` ícone da bandeja, janela e comandos
- `src-tauri/src/session.rs` login (password no cofre do sistema), AC escolhido e pedidos ao hOn
- `src-tauri/src/bridge.rs` comunicação com o `hon-bridge`
- `bridge/` o `hon-bridge` em Python
