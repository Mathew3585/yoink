import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// --- Références DOM ---
const $ = (id) => document.getElementById(id);
const els = {
  // topbar
  tabs: $("tabs"),
  historyCount: $("historyCount"),
  engineBadge: $("engineBadge"),
  engineText: $("engineText"),
  themeBtn: $("themeBtn"),
  logBtn: $("logBtn"),
  // vues
  viewDownload: $("view-download"),
  viewHistory: $("view-history"),
  // téléchargement
  placeholder: $("placeholder"),
  url: $("url"),
  fetchBtn: $("fetchBtn"),
  preview: $("preview"),
  thumb: $("thumb"),
  vidTitle: $("vidTitle"),
  vidMeta: $("vidMeta"),
  options: $("options"),
  fileName: $("fileName"),
  fileExt: $("fileExt"),
  modeSeg: $("modeSeg"),
  qualityGroup: $("qualityGroup"),
  quality: $("quality"),
  audioGroup: $("audioGroup"),
  audioQuality: $("audioQuality"),
  nvenc: $("nvenc"),
  gpuGroup: $("gpuGroup"),
  upscale: $("upscale"),
  upscaleGroup: $("upscaleGroup"),
  upscaleHint: $("upscaleHint"),
  outDir: $("outDir"),
  pickDir: $("pickDir"),
  downloadBtn: $("downloadBtn"),
  progress: $("progress"),
  progLabel: $("progLabel"),
  progPct: $("progPct"),
  barFill: $("barFill"),
  progSpeed: $("progSpeed"),
  progEta: $("progEta"),
  // historique
  historyList: $("historyList"),
  historyEmpty: $("historyEmpty"),
  clearHistory: $("clearHistory"),
  // journal
  logDrawer: $("logDrawer"),
  closeLog: $("closeLog"),
  log: $("log"),
  clearLog: $("clearLog"),
};

// Formats proposés dans le sélecteur d'extension (à droite du nom de fichier).
const VIDEO_FORMATS = ["mp4", "mkv", "webm", "mov"];
const AUDIO_FORMATS = ["mp3", "m4a", "opus", "flac", "wav"];

let state = {
  mode: "video", // "video" | "audio"
  videoFormat: "mp4", // format retenu pour la vidéo
  audioFormat: "mp3", // format retenu pour l'audio
  info: null, // infos vidéo récupérées
  btnMode: "download", // "download" | "cancel" | "open"
  lastDir: "", // dernier dossier de sortie (pour « Ouvrir »)
  lastFile: "", // dernier chemin de fichier complet
};

// ============================================================
//  Thème (clair / sombre) — persisté dans localStorage
// ============================================================
const THEME_KEY = "ytdl.theme";
function applyTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme);
  els.themeBtn.title = theme === "dark" ? "Passer en clair" : "Passer en sombre";
}
function initTheme() {
  const saved = localStorage.getItem(THEME_KEY);
  applyTheme(saved === "dark" ? "dark" : "light");
}
function toggleTheme() {
  const next =
    document.documentElement.getAttribute("data-theme") === "dark"
      ? "light"
      : "dark";
  localStorage.setItem(THEME_KEY, next);
  applyTheme(next);
}

// ============================================================
//  Navigation entre vues
// ============================================================
function setView(view) {
  els.viewDownload.classList.toggle("active", view === "download");
  els.viewHistory.classList.toggle("active", view === "history");
  for (const tab of els.tabs.querySelectorAll(".tab")) {
    tab.classList.toggle("active", tab.dataset.view === view);
  }
  if (view === "history") renderHistory();
}

// ============================================================
//  Historique — persisté dans localStorage
// ============================================================
const HISTORY_KEY = "ytdl.history";
function loadHistory() {
  try {
    return JSON.parse(localStorage.getItem(HISTORY_KEY)) || [];
  } catch {
    return [];
  }
}
function saveHistory(list) {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(list.slice(0, 100)));
  updateHistoryCount(list.length);
}
function updateHistoryCount(n) {
  els.historyCount.textContent = n > 0 ? String(n) : "";
  els.historyCount.dataset.empty = n > 0 ? "false" : "true";
}
function addToHistory(entry) {
  const list = loadHistory();
  list.unshift(entry);
  saveHistory(list);
}

function fmtDate(ts) {
  const d = new Date(ts);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const time = d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
  if (sameDay) return `Aujourd'hui, ${time}`;
  return `${d.toLocaleDateString("fr-FR", { day: "2-digit", month: "short" })}, ${time}`;
}

// Escape pour insertion sûre dans le HTML.
function esc(s) {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const ICON_FOLDER =
  '<svg viewBox="0 0 24 24" width="18" height="18"><path fill="currentColor" d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6Z"/></svg>';
const ICON_REDL =
  '<svg viewBox="0 0 24 24" width="18" height="18"><path fill="currentColor" d="M12 3a1 1 0 0 1 1 1v8.6l2.3-2.3a1 1 0 0 1 1.4 1.4l-4 4a1 1 0 0 1-1.4 0l-4-4a1 1 0 1 1 1.4-1.4l2.3 2.3V4a1 1 0 0 1 1-1Zm-7 15a1 1 0 0 1 1-1h12a1 1 0 1 1 0 2H6a1 1 0 0 1-1-1Z"/></svg>';
const ICON_TRASH =
  '<svg viewBox="0 0 24 24" width="18" height="18"><path fill="currentColor" d="M9 3a1 1 0 0 0-1 1v1H5a1 1 0 0 0 0 2h14a1 1 0 1 0 0-2h-3V4a1 1 0 0 0-1-1H9Zm-2 6a1 1 0 0 1 1 1v7a1 1 0 1 1-2 0v-7a1 1 0 0 1 1-1Zm5 0a1 1 0 0 1 1 1v7a1 1 0 1 1-2 0v-7a1 1 0 0 1 1-1Zm5 0a1 1 0 0 1 1 1v7a1 1 0 1 1-2 0v-7a1 1 0 0 1 1-1ZM6 7h12l-.8 12.1A2 2 0 0 1 15.2 21H8.8a2 2 0 0 1-2-1.9L6 7Z"/></svg>';

function renderHistory() {
  const list = loadHistory();
  updateHistoryCount(list.length);
  els.historyEmpty.classList.toggle("hidden", list.length > 0);
  els.historyList.classList.toggle("hidden", list.length === 0);
  els.historyList.innerHTML = "";

  for (const item of list) {
    const row = document.createElement("div");
    row.className = "history-item";
    const badge = item.format || (item.mode === "audio" ? "MP3" : "MP4");
    const quality = item.quality ? `<span class="chip">${esc(item.quality)}</span>` : "";
    const upscale = item.upscale ? `<span class="chip chip-up">↑ ${esc(item.upscale)}</span>` : "";
    row.innerHTML = `
      <div class="hist-thumb">
        ${item.thumbnail ? `<img src="${esc(item.thumbnail)}" alt="" />` : ""}
        <span class="hist-badge">${badge}</span>
      </div>
      <div class="hist-info">
        <div class="hist-title">${esc(item.title || "(sans titre)")}</div>
        <div class="hist-meta">
          ${quality}
          ${upscale}
          <span>${esc(fmtDate(item.date))}</span>
        </div>
      </div>
      <div class="hist-actions">
        <button class="icon-btn" data-act="open" title="Ouvrir le fichier">${ICON_FOLDER}</button>
        <button class="icon-btn" data-act="redl" title="Retélécharger">${ICON_REDL}</button>
        <button class="icon-btn" data-act="del" title="Retirer de l'historique">${ICON_TRASH}</button>
      </div>`;

    row.querySelector('[data-act="open"]').addEventListener("click", () => {
      invoke("reveal_path", { path: item.filePath || item.dir }).catch((e) =>
        logLine(`Impossible d'ouvrir : ${e}`, "err")
      );
    });
    row.querySelector('[data-act="redl"]').addEventListener("click", () => {
      els.url.value = item.url || "";
      setView("download");
      if (item.url) fetchInfo();
    });
    row.querySelector('[data-act="del"]').addEventListener("click", () => {
      const rest = loadHistory().filter((h) => h.id !== item.id);
      saveHistory(rest);
      renderHistory();
    });

    els.historyList.appendChild(row);
  }
}

// ============================================================
//  Utilitaires UI
// ============================================================
function logLine(text, kind = "") {
  const line = document.createElement("div");
  if (kind) line.style.color = kind === "err" ? "var(--accent)" : "var(--go)";
  line.textContent = text;
  els.log.appendChild(line);
  els.log.scrollTop = els.log.scrollHeight;
}

function fmtDuration(sec) {
  if (!sec || isNaN(sec)) return "";
  sec = Math.round(sec);
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  const pad = (n) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

// Nettoie un nom de fichier des caractères interdits par Windows.
function sanitizeFilename(name) {
  return (name || "")
    .replace(/[<>:"/\\|?*%\x00-\x1f]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 180);
}

// Étiquette lisible pour une hauteur de vidéo donnée.
function qualityLabel(h) {
  if (h >= 4320) return `8K (${h}p)`;
  if (h >= 2160) return `4K (${h}p)`;
  if (h >= 1440) return `2K (${h}p)`;
  if (h >= 1080) return `Full HD (${h}p)`;
  if (h >= 720) return `HD (${h}p)`;
  return `${h}p`;
}

// ============================================================
//  Dropdown custom (progressive enhancement d'un <select>)
//  Le <select> natif reste la source de vérité, on l'habille.
// ============================================================
const CHEVRON =
  '<svg class="dd-chevron" viewBox="0 0 24 24" width="16" height="16"><path fill="currentColor" d="M7 10l5 5 5-5z"/></svg>';

function enhanceSelect(sel, { variant = "default" } = {}) {
  const wrap = document.createElement("div");
  wrap.className = "dd" + (variant === "badge" ? " dd--badge" : "");
  sel.parentNode.insertBefore(wrap, sel);
  wrap.appendChild(sel);
  sel.classList.add("dd-native");

  const trigger = document.createElement("button");
  trigger.type = "button";
  trigger.className = "dd-trigger";
  trigger.innerHTML = `<span class="dd-label"></span>${CHEVRON}`;
  wrap.appendChild(trigger);
  const labelEl = trigger.querySelector(".dd-label");

  const menu = document.createElement("div");
  menu.className = "dd-menu";
  document.body.appendChild(menu);

  let open = false;

  const syncLabel = () => {
    const o = sel.options[sel.selectedIndex];
    labelEl.textContent = o ? o.textContent : "";
  };

  const buildMenu = () => {
    menu.innerHTML = "";
    Array.from(sel.options).forEach((o, i) => {
      const item = document.createElement("div");
      item.className = "dd-item" + (i === sel.selectedIndex ? " active" : "");
      item.textContent = o.textContent;
      item.addEventListener("click", () => {
        if (sel.selectedIndex !== i) {
          sel.selectedIndex = i;
          sel.dispatchEvent(new Event("change", { bubbles: true }));
        }
        syncLabel();
        closeMenu();
      });
      menu.appendChild(item);
    });
  };

  const position = () => {
    const r = trigger.getBoundingClientRect();
    menu.style.minWidth = `${r.width}px`;
    menu.style.left = `${r.left}px`;
    menu.style.top = `${r.bottom + 6}px`;
    const mh = menu.offsetHeight;
    // Bascule vers le haut si pas assez de place en bas.
    if (r.bottom + 6 + mh > window.innerHeight - 8 && r.top - mh - 6 > 8) {
      menu.style.top = `${r.top - mh - 6}px`;
    }
    // Recadre si le menu déborde à droite.
    const mw = menu.offsetWidth;
    if (r.left + mw > window.innerWidth - 8) {
      menu.style.left = `${Math.max(8, window.innerWidth - 8 - mw)}px`;
    }
  };

  const openMenu = () => {
    buildMenu();
    menu.classList.add("open");
    trigger.classList.add("open");
    position();
    const active = menu.querySelector(".dd-item.active");
    if (active) active.scrollIntoView({ block: "nearest" });
    open = true;
    document.addEventListener("mousedown", onDocDown, true);
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("scroll", closeMenu, true);
    window.addEventListener("resize", closeMenu, true);
  };

  const closeMenu = () => {
    if (!open) return;
    menu.classList.remove("open");
    trigger.classList.remove("open");
    open = false;
    document.removeEventListener("mousedown", onDocDown, true);
    document.removeEventListener("keydown", onKey, true);
    window.removeEventListener("scroll", closeMenu, true);
    window.removeEventListener("resize", closeMenu, true);
  };

  const onDocDown = (e) => {
    if (!menu.contains(e.target) && !wrap.contains(e.target)) closeMenu();
  };

  const onKey = (e) => {
    if (e.key === "Escape") return closeMenu();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      let i = sel.selectedIndex + (e.key === "ArrowDown" ? 1 : -1);
      i = Math.max(0, Math.min(sel.options.length - 1, i));
      if (i !== sel.selectedIndex) {
        sel.selectedIndex = i;
        sel.dispatchEvent(new Event("change", { bubbles: true }));
        syncLabel();
        buildMenu();
        const active = menu.querySelector(".dd-item.active");
        if (active) active.scrollIntoView({ block: "nearest" });
      }
    }
    if (e.key === "Enter") {
      e.preventDefault();
      closeMenu();
    }
  };

  trigger.addEventListener("click", () => (open ? closeMenu() : openMenu()));

  syncLabel();
  // Exposé pour resynchroniser après repopulation des options.
  sel._dd = {
    refresh() {
      syncLabel();
      if (open) {
        buildMenu();
        position();
      }
    },
  };
}

// ============================================================
//  Vérification des moteurs au démarrage
// ============================================================
async function checkEngines() {
  try {
    const res = await invoke("check_engines");
    if (res.yt_dlp && res.ffmpeg) {
      els.engineText.textContent = `Prêt · yt-dlp ${res.yt_dlp}`;
      els.engineBadge.className = "badge ok";
    } else {
      const missing = [];
      if (!res.yt_dlp) missing.push("yt-dlp");
      if (!res.ffmpeg) missing.push("ffmpeg");
      els.engineText.textContent = `Manquant : ${missing.join(", ")}`;
      els.engineBadge.className = "badge err";
      logLine(
        `⚠ Binaires manquants : ${missing.join(", ")}. Lance le script d'installation.`,
        "err"
      );
    }
  } catch (e) {
    els.engineText.textContent = "Erreur moteurs";
    els.engineBadge.className = "badge err";
    logLine(`Erreur de vérification : ${e}`, "err");
  }
}

// ============================================================
//  Analyse de l'URL
// ============================================================
async function fetchInfo() {
  const url = els.url.value.trim();
  if (!url) {
    logLine("Colle d'abord un lien YouTube.", "err");
    els.url.focus();
    return;
  }
  els.fetchBtn.disabled = true;
  els.fetchBtn.textContent = "Analyse…";
  logLine(`Analyse de ${url}`);

  setBtnMode("download");
  els.placeholder.classList.add("hidden");
  els.options.classList.add("hidden");
  els.thumb.removeAttribute("src");
  els.vidTitle.textContent = "Analyse en cours…";
  els.vidMeta.textContent = "Récupération des informations";
  els.preview.classList.add("loading");
  els.preview.classList.remove("hidden");

  try {
    const info = await invoke("fetch_info", { url });
    state.info = info;

    els.thumb.src = info.thumbnail || "";
    els.vidTitle.textContent = info.title || "(sans titre)";
    const meta = [];
    if (info.uploader) meta.push(info.uploader);
    if (info.duration) meta.push(fmtDuration(info.duration));
    els.vidMeta.textContent = meta.join("  ·  ") || "—";
    els.preview.classList.remove("loading");

    els.fileName.value = sanitizeFilename(info.title);

    els.quality.innerHTML = "";
    const heights = info.heights || [];
    if (heights.length === 0) {
      const opt = document.createElement("option");
      opt.value = "9999";
      opt.textContent = "Meilleure qualité disponible";
      els.quality.appendChild(opt);
    } else {
      for (const h of heights) {
        const opt = document.createElement("option");
        opt.value = String(h);
        opt.textContent = qualityLabel(h);
        els.quality.appendChild(opt);
      }
    }

    els.quality._dd?.refresh();
    els.options.classList.remove("hidden");
    logLine(`✓ Vidéo analysée : « ${info.title} »`, "ok");
  } catch (e) {
    state.info = null;
    els.preview.classList.add("hidden");
    els.preview.classList.remove("loading");
    els.placeholder.classList.remove("hidden");
    logLine(`✗ Impossible d'analyser : ${e}`, "err");
    showLog(true);
  } finally {
    els.fetchBtn.disabled = false;
    els.fetchBtn.textContent = "Analyser";
  }
}

// --- Sélection du dossier (renvoie le dossier choisi, ou null si annulé) ---
async function pickDir() {
  const dir = await open({ directory: true, multiple: false, title: "Choisir le dossier de sortie" });
  if (dir) {
    els.outDir.value = dir;
    localStorage.setItem("ytdl.outDir", dir);
  }
  return dir || null;
}

// Extension de fichier courante (= format choisi dans le badge déroulant).
function currentExt() {
  return els.fileExt.value;
}

// (Re)remplit le sélecteur d'extension selon le mode, en conservant le dernier
// format choisi pour ce mode.
function populateFormats() {
  const list = state.mode === "video" ? VIDEO_FORMATS : AUDIO_FORMATS;
  const current = state.mode === "video" ? state.videoFormat : state.audioFormat;
  els.fileExt.innerHTML = "";
  for (const f of list) {
    const opt = document.createElement("option");
    opt.value = f;
    opt.textContent = `.${f}`;
    els.fileExt.appendChild(opt);
  }
  els.fileExt.value = list.includes(current) ? current : list[0];
  els.fileExt._dd?.refresh();
}

// Mémorise le format choisi pour le mode courant.
function rememberFormat() {
  if (state.mode === "video") state.videoFormat = els.fileExt.value;
  else state.audioFormat = els.fileExt.value;
}

// Affiche l'avertissement uniquement quand un agrandissement IA est choisi.
function updateUpscaleHint() {
  const ai = els.upscale.value === "ai2" || els.upscale.value === "ai4";
  els.upscaleHint.classList.toggle("hidden", !ai);
}

// --- Bascule vidéo / audio ---
function setMode(mode) {
  state.mode = mode;
  for (const btn of els.modeSeg.querySelectorAll(".seg")) {
    btn.classList.toggle("active", btn.dataset.mode === mode);
  }
  const isVideo = mode === "video";
  els.qualityGroup.classList.toggle("hidden", !isVideo);
  els.gpuGroup.classList.toggle("hidden", !isVideo);
  els.upscaleGroup.classList.toggle("hidden", !isVideo);
  els.audioGroup.classList.toggle("hidden", isVideo);
  els.upscaleHint.classList.toggle("hidden", !isVideo || els.upscale.value === "none" || els.upscale.value === "fast");
  populateFormats();
  if (state.btnMode === "open") resetBtn();
}

// ============================================================
//  Bouton principal : télécharger / annuler / ouvrir
// ============================================================
async function onMainButton() {
  if (state.btnMode === "cancel") {
    await invoke("cancel_download").catch(() => {});
    logLine("Annulation demandée…");
    return;
  }
  if (state.btnMode === "open") {
    await invoke("reveal_path", { path: state.lastFile || state.lastDir }).catch((e) =>
      logLine(`Impossible d'ouvrir le dossier : ${e}`, "err")
    );
    return;
  }

  // Mode « download »
  if (!state.info) {
    logLine("Analyse d'abord une vidéo.", "err");
    return;
  }
  // Pas de dossier choisi ? On ouvre directement le sélecteur, puis on continue.
  if (!els.outDir.value) {
    const dir = await pickDir();
    if (!dir) return; // l'utilisateur a annulé
  }
  const fileName =
    sanitizeFilename(els.fileName.value) ||
    sanitizeFilename(state.info.title) ||
    "video";

  const opts = {
    url: els.url.value.trim(),
    out_dir: els.outDir.value,
    file_name: fileName,
    mode: state.mode,
    max_height: state.mode === "video" ? parseInt(els.quality.value, 10) : 0,
    audio_quality: els.audioQuality.value,
    video_format: state.videoFormat,
    audio_format: state.audioFormat,
    nvenc: state.mode === "video" ? els.nvenc.value : "none",
    upscale: state.mode === "video" ? els.upscale.value : "none",
  };

  // Mémorise le contexte pour l'historique (au succès).
  const ext = currentExt();
  const sep = els.outDir.value.includes("/") ? "/" : "\\";
  state.lastDir = els.outDir.value;
  state.lastFile = `${els.outDir.value}${sep}${fileName}.${ext}`;
  state.pending = {
    id: `${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
    url: opts.url,
    title: state.info.title,
    thumbnail: state.info.thumbnail,
    uploader: state.info.uploader,
    mode: state.mode,
    format: ext.toUpperCase(),
    upscale:
      state.mode === "video" && els.upscale.value !== "none"
        ? { ai2: "IA ×2", ai4: "IA ×4", fast: "×2" }[els.upscale.value] || ""
        : "",
    quality:
      state.mode === "video"
        ? els.quality.options[els.quality.selectedIndex]?.textContent || ""
        : els.audioQuality.options[els.audioQuality.selectedIndex]?.textContent || "",
    dir: state.lastDir,
    filePath: state.lastFile,
  };

  setBtnMode("cancel");
  els.fetchBtn.disabled = true;
  els.progress.classList.remove("hidden");
  resetProgress();
  logLine(`▶ Téléchargement lancé (${state.mode === "video" ? "MP4" : "MP3"})…`);

  try {
    await invoke("start_download", { opts });
  } catch (e) {
    logLine(`✗ Échec : ${e}`, "err");
    showLog(true);
    setBtnMode("download");
    els.fetchBtn.disabled = false;
  }
}

// Met à jour l'apparence du bouton principal selon l'état.
function setBtnMode(mode) {
  state.btnMode = mode;
  const b = els.downloadBtn;
  b.classList.remove("btn-go", "btn-cancel", "btn-open");
  if (mode === "cancel") {
    b.textContent = "Annuler";
    b.classList.add("btn-cancel");
  } else if (mode === "open") {
    b.innerHTML = `${ICON_FOLDER} Ouvrir le fichier`;
    b.classList.add("btn-open");
  } else {
    b.textContent = "Télécharger";
    b.classList.add("btn-go");
  }
}

function resetBtn() {
  setBtnMode("download");
  els.fetchBtn.disabled = false;
}

function resetProgress() {
  els.barFill.style.width = "0%";
  els.barFill.classList.remove("done");
  els.progPct.textContent = "0 %";
  els.progLabel.textContent = "Préparation…";
  els.progSpeed.textContent = "";
  els.progEta.textContent = "";
}

// ============================================================
//  Écoute des events Rust
// ============================================================
listen("dl:progress", (e) => {
  const { percent, speed, eta, phase } = e.payload;
  if (percent != null) {
    els.barFill.style.width = `${percent}%`;
    els.progPct.textContent = `${percent.toFixed(1)} %`;
  }
  if (phase) els.progLabel.textContent = phase;
  if (speed != null) els.progSpeed.textContent = speed;
  if (eta != null) els.progEta.textContent = `ETA ${eta}`;
});

listen("dl:log", (e) => {
  logLine(e.payload);
});

listen("dl:done", (e) => {
  const ok = e.payload.success;
  els.fetchBtn.disabled = false;
  if (ok) {
    els.barFill.style.width = "100%";
    els.barFill.classList.add("done");
    els.progPct.textContent = "100 %";
    els.progLabel.textContent = "Terminé ✓";
    els.progSpeed.textContent = "";
    els.progEta.textContent = "";
    logLine(`✓ Téléchargement terminé → ${state.lastDir}`, "ok");
    setBtnMode("open");
    // Enregistre dans l'historique
    if (state.pending) {
      addToHistory({ ...state.pending, date: Date.now() });
      state.pending = null;
    }
  } else {
    els.progLabel.textContent = "Arrêté";
    logLine(`✗ ${e.payload.message || "Téléchargement interrompu."}`, "err");
    showLog(true);
    setBtnMode("download");
    state.pending = null;
  }
});

// ============================================================
//  Journal (tiroir basculable)
// ============================================================
function showLog(show) {
  els.logDrawer.classList.toggle("hidden", !show);
  els.logBtn.classList.toggle("active", show);
}
function toggleLog() {
  showLog(els.logDrawer.classList.contains("hidden"));
}

// ============================================================
//  Branchements
// ============================================================
els.tabs.addEventListener("click", (e) => {
  const tab = e.target.closest(".tab");
  if (tab) setView(tab.dataset.view);
});
els.themeBtn.addEventListener("click", toggleTheme);
els.fetchBtn.addEventListener("click", fetchInfo);
els.url.addEventListener("keydown", (e) => {
  if (e.key === "Enter") fetchInfo();
});
els.pickDir.addEventListener("click", pickDir);
els.downloadBtn.addEventListener("click", onMainButton);
els.clearLog.addEventListener("click", () => (els.log.innerHTML = ""));
els.logBtn.addEventListener("click", toggleLog);
els.closeLog.addEventListener("click", () => showLog(false));
els.clearHistory.addEventListener("click", () => {
  if (loadHistory().length === 0) return;
  saveHistory([]);
  renderHistory();
});
els.modeSeg.addEventListener("click", (e) => {
  const btn = e.target.closest(".seg");
  if (btn) setMode(btn.dataset.mode);
});
els.fileExt.addEventListener("change", rememberFormat);
els.upscale.addEventListener("change", updateUpscaleHint);

// ============================================================
//  Démarrage
// ============================================================
initTheme();
// Habille tous les <select> en dropdowns custom (avant setMode/populateFormats).
enhanceSelect(els.fileExt, { variant: "badge" });
enhanceSelect(els.quality);
enhanceSelect(els.audioQuality);
enhanceSelect(els.nvenc);
enhanceSelect(els.upscale);
setMode("video");
checkEngines();
updateHistoryCount(loadHistory().length);
const savedDir = localStorage.getItem("ytdl.outDir");
if (savedDir) els.outDir.value = savedDir;
