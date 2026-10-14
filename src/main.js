// Interface do painel. Fala com o backend Rust (que fala com o hOn).
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
    ["auto", "Auto"],
    ["top", "Cima"],
    ["middle", "Meio"],
    ["bottom", "Baixo"],
  ],
};

const ERRORS = {
  auth_failed: "Email ou password errados.",
  bridge_closed: "Perdi a ligação ao hOn. Tenta outra vez.",
  command_rejected: "O AC recusou o comando.",
  no_ac: "Não há nenhum AC nesta conta.",
};
const errorText = (e) => ERRORS[String(e)] ?? `Erro: ${e}`;

const tauri = window.__TAURI__;
const invoke = tauri ? (cmd, args) => tauri.core.invoke(cmd, args) : mockInvoke();

const $ = (id) => document.getElementById(id);
const panel = $("panel");
const menu = $("menu");
let session = null; // { loggedIn, email, acs, selected }
let state = null; // estado do AC escolhido

// ---------- desenhar ----------

// Cores do fundo nos extremos: 16 °C (frio, azul) e 30 °C (quente, laranja).
const COLD = { top: [30, 52, 86], mid: [19, 34, 58], bottom: [11, 20, 36], glow: [90, 150, 255] };
const WARM = { top: [59, 40, 34], mid: [42, 29, 25], bottom: [27, 20, 18], glow: [255, 140, 70] };
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
const rgb = (c) => `rgb(${c.join(",")})`;

// Mistura o fundo entre azul e laranja conforme a temperatura escolhida.
function paintTemperature(temp) {
  const min = state?.minTemp ?? 16;
  const max = state?.maxTemp ?? 30;
  const t = Math.min(1, Math.max(0, (temp - min) / (max - min)));
  const style = panel.style;
  style.setProperty("--bg-top", rgb(mix(COLD.top, WARM.top, t)));
  style.setProperty("--bg-mid", rgb(mix(COLD.mid, WARM.mid, t)));
  style.setProperty("--bg-bottom", rgb(mix(COLD.bottom, WARM.bottom, t)));
  style.setProperty("--bg-glow", `rgba(${mix(COLD.glow, WARM.glow, t).join(",")}, 0.16)`);
  // O preenchimento do slider vai até ao centro do botão (30px de largura).
  style.setProperty("--fill", `calc(15px + (100% - 30px) * ${t})`);
}

function label(key, value) {
  if (key === "sleep") return value ? "On" : "Off";
  return OPTIONS[key].find(([v]) => v === value)?.[1] ?? value;
}

function showLoading(text) {
  $("loadingText").textContent = text;
  $("loading").hidden = false;
  $("login").hidden = true;
}

function showLogin(error = "") {
  $("loading").hidden = true;
  $("login").hidden = false;
  $("loginError").textContent = error;
  $("loginButton").disabled = false;
  if (session?.email && !$("email").value) $("email").value = session.email;
}

function showPanel() {
  $("loading").hidden = true;
  $("login").hidden = true;
}

function render() {
  if (!state) return;
  const features = state.features ?? [];
  panel.classList.toggle("on", state.power);
  panel.classList.toggle("offline", state.online === false);
  panel.dataset.mode = state.mode;
  $("acName").textContent = state.name;
  $("roomTemp").textContent = state.roomTemp ? `${Math.round(state.roomTemp)}°C` : "--°C";
  $("targetTemp").textContent = state.targetTemp;
  $("unitTemp").textContent = state.power ? state.targetTemp : "--";
  const slider = $("slider");
  slider.min = state.minTemp ?? 16;
  slider.max = state.maxTemp ?? 30;
  slider.value = state.targetTemp;
  paintTemperature(state.targetTemp);
  document.querySelector(".slider-labels").innerHTML =
    `<span>${slider.min}°</span><span>${slider.max}°</span>`;
  $("eco").hidden = !features.includes("eco");
  $("eco").setAttribute("aria-checked", String(!!state.eco));
  let tiles = 2;
  for (const key of ["sleep", "airflow"]) {
    const has = features.includes(key);
    document.querySelector(`.tile[data-key="${key}"]`).hidden = !has;
    tiles += has ? 1 : 0;
  }
  const grid = document.querySelector(".tiles");
  grid.classList.toggle("three", tiles === 3);
  grid.classList.toggle("two", tiles === 2);
  for (const el of document.querySelectorAll("[data-value]")) {
    const key = el.dataset.value;
    el.textContent = label(key, state[key]);
  }
  document.querySelector('.tile[data-key="sleep"]').dataset.active = String(!!state.sleep);
  $("acPicker").disabled = (session?.acs?.length ?? 0) < 2;
  $("acPicker").querySelector(".chev-inline").style.display = $("acPicker").disabled ? "none" : "";
}

let messageTimer = null;
function message(text) {
  $("message").textContent = text;
  clearTimeout(messageTimer);
  if (text) messageTimer = setTimeout(() => ($("message").textContent = ""), 6000);
}

// ---------- falar com o backend ----------

async function refresh() {
  if (!session?.loggedIn || Object.keys(pending).length || sending) return;
  try {
    state = await invoke("get_state");
    render();
    showPanel();
  } catch (e) {
    if (String(e) === "auth_failed" || String(e) === "no_saved_login") return showLogin(errorText(e));
    if (!state) {
      // Ainda não há nada para mostrar: fica no ecrã de espera e tenta de novo.
      showLoading(`${errorText(e)} A tentar outra vez…`);
      setTimeout(refresh, 10000);
      return;
    }
    message(errorText(e));
  }
}

async function start() {
  showLoading("A ligar ao hOn…");
  session = await invoke("status");
  if (!session.loggedIn) return showLogin();
  showLoading("A ler o estado do AC…");
  await refresh();
}

let pending = {};
let sendTimer = null;
let sending = false;

// Atualiza já o ecrã e envia para o AC. Mudanças seguidas (como arrastar o
// slider) são juntadas num só pedido.
function change(patch, delay = 0) {
  if (!state) return;
  // Mudar qualquer definição com o AC desligado liga-o, tal como na app hOn.
  if (!state.power && patch.power === undefined) patch = { ...patch, power: true };
  state = { ...state, ...patch };
  render();
  pending = { ...pending, ...patch };
  clearTimeout(sendTimer);
  sendTimer = setTimeout(flush, delay);
}

async function flush() {
  if (sending) return (sendTimer = setTimeout(flush, 200));
  const patch = pending;
  pending = {};
  sending = true;
  try {
    const next = await invoke("update_state", { patch });
    if (Object.keys(pending).length === 0) state = next;
    message("");
  } catch (err) {
    message(errorText(err));
  } finally {
    sending = false;
  }
  if (Object.keys(pending).length === 0) render();
}

function setTemp(t) {
  const temp = Math.min(state.maxTemp ?? 30, Math.max(state.minTemp ?? 16, t));
  if (temp !== state.targetTemp) change({ targetTemp: temp }, 600);
}

// ---------- eventos ----------

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
    openMenu(tile, OPTIONS[key], state[key], (value) => change({ [key]: value }), "above");
  });
}

$("acPicker").addEventListener("click", (e) => {
  e.stopPropagation();
  if (!session?.acs?.length) return;
  if ($("acPicker").classList.contains("open")) return closeMenu();
  const items = session.acs.map((ac) => [ac.id, ac.name]);
  openMenu($("acPicker"), items, session.selected, selectAc, "below");
});

async function selectAc(id) {
  if (id === session.selected) return;
  session = await invoke("select_ac", { id });
  state = null;
  showLoading("A ler o estado do AC…");
  await refresh();
}

$("login").addEventListener("submit", async (e) => {
  e.preventDefault();
  $("loginButton").disabled = true;
  $("loginError").textContent = "";
  showLoading("A entrar no hOn…");
  try {
    session = await invoke("login", { email: $("email").value.trim(), password: $("password").value });
    $("password").value = "";
    showLoading("A ler o estado do AC…");
    await refresh();
  } catch (err) {
    showLogin(errorText(err));
  }
});

$("logout").addEventListener("click", async () => {
  await invoke("logout");
  session = { loggedIn: false, email: null, acs: [], selected: null };
  state = null;
  $("email").value = "";
  showLogin();
});

function openMenu(anchor, items, current, onPick, where) {
  closeMenu();
  anchor.classList.add("open");
  menu.replaceChildren(
    ...items.map(([value, text]) => {
      const b = document.createElement("button");
      b.textContent = text;
      b.setAttribute("aria-selected", String(current === value));
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        closeMenu();
        onPick(value);
      });
      return b;
    }),
  );
  menu.hidden = false;
  const p = panel.getBoundingClientRect();
  const a = anchor.getBoundingClientRect();
  const left = Math.min(a.left - p.left, p.width - menu.offsetWidth - 10);
  menu.style.left = `${Math.max(10, left)}px`;
  menu.style.top =
    where === "below"
      ? `${a.bottom - p.top + 6}px`
      : `${a.top - p.top - menu.offsetHeight - 6}px`;
}

function closeMenu() {
  menu.hidden = true;
  document.querySelectorAll(".open").forEach((t) => t.classList.remove("open"));
}

document.addEventListener("click", closeMenu);
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (!menu.hidden) return closeMenu();
  if (tauri) invoke("hide_panel");
});

// Volta a ler o estado quando o painel abre e de 30 em 30 segundos enquanto está aberto.
window.addEventListener("focus", refresh);
setInterval(() => {
  if (document.hasFocus()) refresh();
}, 30000);

// ---------- AC simulado para ver o design no browser ----------

function mockInvoke() {
  const acs = [
    { id: "a", name: "Tomás AC", model: "AS25" },
    { id: "b", name: "Livingroom AC", model: "AS35" },
  ];
  let s = { loggedIn: true, email: "teste@exemplo.pt", acs, selected: "a" };
  const states = Object.fromEntries(
    acs.map((ac, i) => [ac.id, {
      name: ac.name, online: true, power: i === 0, mode: "cool", targetTemp: 22 + i,
      roomTemp: 25 + i, fanSpeed: "auto", sleep: false, airflow: "swing",
      minTemp: 16, maxTemp: 30, features: ["sleep", "airflow"],
    }]),
  );
  return async (cmd, args) => {
    switch (cmd) {
      case "status": return { ...s };
      case "login": s = { ...s, loggedIn: true, email: args.email }; return { ...s };
      case "logout": s = { ...s, loggedIn: false }; return null;
      case "select_ac": s = { ...s, selected: args.id }; return { ...s };
      case "get_state": return { ...states[s.selected] };
      case "update_state": Object.assign(states[s.selected], args.patch); return { ...states[s.selected] };
    }
  };
}

start();
