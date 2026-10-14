// Interface do painel. Fala com o backend Rust através de `get_state` e `update_state`.
// Aberta fora do Tauri (num browser), usa um AC simulado para se poder ver o design.

const OPTIONS = {
  mode: [
    ["auto", "Auto"],
    ["cool", "Frio"],
    ["dry", "Desumid."],
    ["heat", "Calor"],
    ["fan", "Ventilar"],
  ],
  fanSpeed: [
    ["auto", "Auto"],
    ["low", "Baixa"],
    ["mid", "Média"],
    ["high", "Alta"],
  ],
  airflow: [
    ["swing", "Oscilar"],
    ["fixed", "Fixo"],
  ],
};

const tauri = window.__TAURI__;
const invoke = tauri
  ? (cmd, args) => tauri.core.invoke(cmd, args)
  : mockInvoke();

const $ = (id) => document.getElementById(id);
const panel = $("panel");
const menu = $("menu");
let state = null;

function label(key, value) {
  if (key === "sleep") return value ? "On" : "Off";
  return OPTIONS[key].find(([v]) => v === value)?.[1] ?? value;
}

function render() {
  if (!state) return;
  panel.classList.toggle("on", state.power);
  panel.dataset.mode = state.mode;
  $("roomTemp").textContent = `${Math.round(state.roomTemp)}°C`;
  $("targetTemp").textContent = state.targetTemp;
  $("unitTemp").textContent = state.power ? state.targetTemp : "--";
  $("slider").value = state.targetTemp;
  $("eco").setAttribute("aria-checked", String(state.eco));
  $("acName").textContent = state.name;
  for (const el of document.querySelectorAll("[data-value]")) {
    const key = el.dataset.value;
    el.textContent = label(key, state[key]);
  }
  document.querySelector('.tile[data-key="sleep"]').dataset.active = String(state.sleep);
}

let pending = {};
let sendTimer = null;

// Atualiza já o ecrã e envia para o backend. Mudanças seguidas (como arrastar o
// slider) são juntadas num só pedido.
function change(patch, delay = 0) {
  state = { ...state, ...patch };
  render();
  pending = { ...pending, ...patch };
  clearTimeout(sendTimer);
  sendTimer = setTimeout(flush, delay);
}

async function flush() {
  const patch = pending;
  pending = {};
  try {
    state = await invoke("update_state", { patch });
  } catch (err) {
    console.error("Falhou o envio para o AC", err);
    state = await invoke("get_state");
  }
  if (Object.keys(pending).length === 0) render();
}

function setTemp(t) {
  const temp = Math.min(30, Math.max(16, t));
  if (temp !== state.targetTemp) change({ targetTemp: temp }, 400);
}

$("power").addEventListener("click", () => change({ power: !state.power }));
$("eco").addEventListener("click", () => change({ eco: !state.eco }));
$("minus").addEventListener("click", () => setTemp(state.targetTemp - 1));
$("plus").addEventListener("click", () => setTemp(state.targetTemp + 1));
$("slider").addEventListener("input", (e) => setTemp(Number(e.target.value)));

for (const tile of document.querySelectorAll(".tile")) {
  tile.addEventListener("click", (e) => {
    e.stopPropagation();
    const key = tile.dataset.key;
    if (key === "sleep") return change({ sleep: !state.sleep });
    if (tile.classList.contains("open")) return closeMenu();
    openMenu(tile, key);
  });
}

function openMenu(tile, key) {
  closeMenu();
  tile.classList.add("open");
  menu.replaceChildren(
    ...OPTIONS[key].map(([value, text]) => {
      const b = document.createElement("button");
      b.textContent = text;
      b.setAttribute("aria-selected", String(state[key] === value));
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        closeMenu();
        change({ [key]: value });
      });
      return b;
    }),
  );
  menu.hidden = false;
  const p = panel.getBoundingClientRect();
  const t = tile.getBoundingClientRect();
  const left = Math.min(t.left - p.left, p.width - menu.offsetWidth - 10);
  menu.style.left = `${Math.max(10, left)}px`;
  menu.style.top = `${t.top - p.top - menu.offsetHeight - 6}px`;
}

function closeMenu() {
  menu.hidden = true;
  document.querySelectorAll(".tile.open").forEach((t) => t.classList.remove("open"));
}

document.addEventListener("click", closeMenu);
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (!menu.hidden) return closeMenu();
  if (tauri) invoke("hide_panel");
});

// Volta a ler o estado quando o painel é aberto.
window.addEventListener("focus", async () => {
  state = await invoke("get_state");
  render();
});

function mockInvoke() {
  let mock = {
    name: "Tomás AC", power: true, mode: "cool", targetTemp: 22, roomTemp: 25,
    fanSpeed: "auto", eco: false, sleep: false, airflow: "swing",
  };
  return async (cmd, args) => {
    if (cmd === "update_state") mock = { ...mock, ...args.patch };
    return { ...mock };
  };
}

(async () => {
  state = await invoke("get_state");
  render();
})();
