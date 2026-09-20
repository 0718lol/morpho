import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";
import { applyStatic, lang, setLang, t } from "./i18n";

const isTauri = "__TAURI_INTERNALS__" in window;

interface MatrixEntry {
  format: string;
  category: string;
  targets: string[];
}
interface FileItem {
  path: string;
  name: string;
  size: number;
  fmt: string | null;
  thumb?: string;
}
interface JobView {
  id: number;
  name: string;
  target: string;
  ratio: number;
  status: "queued" | "running" | "done" | "failed" | "cancelled";
  note: string;
  output?: string;
  error?: string;
}

const $ = <T extends HTMLElement>(sel: string) => document.querySelector<T>(sel)!;

const dropzone = $("#dropzone");
const fileInput = $("#file-input") as HTMLInputElement;
const fileList = $("#file-list");
const convertPanel = $("#convert-panel");
const convertBtn = $("#convert-btn") as HTMLButtonElement;
const fileCount = $("#file-count");
const categoryTabs = $("#category-tabs");
const formatPills = $("#format-pills");
const qualityRow = $("#quality-row");
const qualitySlider = $("#quality-slider") as HTMLInputElement;
const qualityValue = $("#quality-value");
const presetSelect = $("#preset-select") as HTMLSelectElement;
const outputLabel = $("#output-label");
const pickOutput = $("#pick-output");
const clearOutput = $("#clear-output");
const queueSection = $("#queue");
const queueList = $("#queue-list");
const historySection = $("#history");
const historyList = $("#history-list");
const clearHistoryBtn = $("#clear-history");
const histTabRecent = $("#hist-tab-recent");
const histTabArchive = $("#hist-tab-archive");
const engineStatus = $("#engine-status");
const statusText = $("#status-text");

let files: FileItem[] = [];
let matrix = new Map<string, MatrixEntry>();
let activeCategory: string | null = null;
let selectedTarget: string | null = null;
let outputDir: string | null = null;
const jobs = new Map<number, JobView>();

const fmtName = (f: string) => t(`cat${f[0].toUpperCase()}${f.slice(1)}`);

function humanSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} MB`;
  return `${(n / 1073741824).toFixed(2)} GB`;
}

function basename(p: string): string {
  const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
  return i >= 0 ? p.slice(i + 1) : p;
}

function extOf(p: string): string {
  const name = basename(p);
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

/* ---------------- files ---------------- */

function addPaths(paths: string[]) {
  let added = 0;
  for (const p of paths) {
    const ext = extOf(p);
    if (!matrix.has(ext)) {
      status(t("browserHint").replace("浏览器预览模式：转换功能需在桌面应用中使用", `跳过不支持的文件: ${basename(p)}`));
      continue;
    }
    if (files.some((f) => f.path === p)) continue;
    const item: FileItem = { path: p, name: basename(p), size: 0, fmt: ext };
    files.push(item);
    added++;
    if (isImage(ext)) {
      invoke<string | null>("thumbnail", { path: p })
        .then((thumb) => {
          if (thumb) {
            item.thumb = thumb;
            renderFiles();
          }
        })
        .catch(() => {});
    }
  }
  if (added > 0) {
    filesChanged();
  }
}

const isImage = (fmt: string) => matrix.get(fmt)?.category === "image";

function filesChanged() {
  renderFiles();
  renderTargets();
  convertPanel.classList.toggle("hidden", files.length === 0);
  fileCount.textContent = String(files.length);
}

function renderFiles() {
  fileList.innerHTML = "";
  fileList.classList.toggle("hidden", files.length === 0);
  for (const f of files) {
    const chip = document.createElement("div");
    chip.className = "file-chip";
    const media = f.thumb
      ? `<img class="thumb" src="${f.thumb}" alt=""/>`
      : `<div class="thumb-fallback">${iconFor(f.fmt)}</div>`;
    chip.innerHTML = `
      ${media}
      <div class="meta">
        <div class="name" title="${escapeHtml(f.name)}">${escapeHtml(f.name)}</div>
        <div class="size">${f.size ? humanSize(f.size) : escapeHtml(f.fmt ?? "?")}</div>
      </div>
      <button class="remove" title="${t("remove")}">✕</button>`;
    chip.querySelector(".remove")!.addEventListener("click", () => {
      files = files.filter((x) => x.path !== f.path);
      filesChanged();
    });
    fileList.appendChild(chip);
  }
}

function iconFor(fmt: string | null): string {
  const cat = fmt ? matrix.get(fmt)?.category : undefined;
  switch (cat) {
    case "video": return "🎬";
    case "audio": return "🎵";
    case "document": return "📄";
    case "sheet": return "📊";
    case "slide": return "📽️";
    case "pdf": return "📕";
    case "text": return "📝";
    default: return "🖼️";
  }
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
}

/* ---------------- target picking ---------------- */

function commonTargets(): Map<string, string[]> {
  // category -> formats in that category supported by every selected file
  const result = new Map<string, Set<string>>();
  files.forEach((f, i) => {
    const entry = matrix.get(f.fmt!);
    if (!entry) return;
    const byCat = new Map<string, Set<string>>();
    for (const tg of entry.targets) {
      const cat = matrix.get(tg)!.category;
      if (!byCat.has(cat)) byCat.set(cat, new Set());
      byCat.get(cat)!.add(tg);
    }
    if (i === 0) {
      for (const [cat, set] of byCat) result.set(cat, new Set(set));
    } else {
      for (const [cat, set] of result) {
        const other = byCat.get(cat);
        if (!other) { result.delete(cat); continue; }
        for (const v of [...set]) if (!other.has(v)) set.delete(v);
        if (set.size === 0) result.delete(cat);
      }
    }
  });
  const out = new Map<string, string[]>();
  for (const [cat, set] of result) out.set(cat, [...set]);
  return out;
}

function renderTargets() {
  categoryTabs.innerHTML = "";
  formatPills.innerHTML = "";
  if (files.length === 0) return;

  const common = commonTargets();
  if (common.size === 0) {
    formatPills.innerHTML = `<span class="output-label">${t("noTargets")}</span>`;
    convertBtn.disabled = true;
    return;
  }
  const cats = [...common.keys()];
  if (!activeCategory || !cats.includes(activeCategory)) activeCategory = cats[0];
  if (!selectedTarget || !common.get(activeCategory)?.includes(selectedTarget)) {
    selectedTarget = common.get(activeCategory)![0];
  }

  for (const cat of cats) {
    const btn = document.createElement("button");
    btn.textContent = fmtName(cat);
    btn.classList.toggle("active", cat === activeCategory);
    btn.addEventListener("click", () => {
      activeCategory = cat;
      selectedTarget = common.get(cat)![0];
      renderTargets();
    });
    categoryTabs.appendChild(btn);
  }

  for (const fmt of common.get(activeCategory)!) {
    const btn = document.createElement("button");
    btn.textContent = pillLabel(fmt);
    btn.classList.toggle("active", fmt === selectedTarget);
    btn.addEventListener("click", () => {
      selectedTarget = fmt;
      renderTargets();
    });
    formatPills.appendChild(btn);
  }
  updateQualityRow();
  convertBtn.disabled = false;
}

function updateQualityRow() {
  const fmt = selectedTarget ?? "";
  const qualityMatters = ["jpg", "webp", "avif"].includes(fmt) || videoOrAudio(fmt);
  qualityRow.classList.toggle("hidden", !qualityMatters);
  presetSelect.classList.toggle("hidden", !isVideo(fmt));
}

const isVideo = (fmt: string) => matrix.get(fmt)?.category === "video";
const isAudioFmt = (fmt: string) => matrix.get(fmt)?.category === "audio";
function videoOrAudio(fmt: string) { return isVideo(fmt) || isAudioFmt(fmt); }

/** Wire names vs. human labels for special pseudo formats. */
function pillLabel(fmt: string): string {
  if (fmt === "searchablepdf") return t("searchablePdf");
  return fmt;
}

/* ---------------- conversion ---------------- */

async function convert() {
  if (!selectedTarget || files.length === 0 || !isTauri) return;
  const quality = qualityRow.classList.contains("hidden")
    ? null
    : parseInt(qualitySlider.value, 10);
  const preset = presetSelect.classList.contains("hidden") || presetSelect.value === ""
    ? null
    : presetSelect.value;
  try {
    await invoke<number[]>("submit_jobs", {
      files: files.map((f) => f.path),
      target: selectedTarget,
      outputDir,
      quality: Number.isNaN(quality as number) ? null : quality,
      preset,
    });
    files = [];
    filesChanged();
    queueSection.classList.remove("hidden");
  } catch (e) {
    status(`submit failed: ${e}`);
  }
}

function renderQueue() {
  queueList.innerHTML = "";
  for (const job of jobs.values()) {
    const card = document.createElement("div");
    card.className = `job-card ${job.status}`;
    const pct = Math.round(job.ratio * 100);
    const statusLabel =
      job.status === "done" ? `✔ ${t("done")}` :
      job.status === "failed" ? `✖ ${t("failed")}${job.error ? ` — ${job.error}` : ""}` :
      job.status === "cancelled" ? t("cancelled") :
      job.status === "queued" ? t("waiting") :
      `${t("converting")} ${pct}%${job.note ? ` — ${job.note}` : ""}`;
    card.innerHTML = `
      <span class="job-name" title="${escapeHtml(job.name)}">${escapeHtml(job.name)}</span>
      <span class="job-target">${escapeHtml(job.target)}</span>
      <div class="job-bar"><div class="fill" style="width:${job.status === "done" ? 100 : pct}%"></div></div>
      <span class="job-status" title="${escapeHtml(statusLabel)}">${escapeHtml(statusLabel)}</span>`;
    const action = document.createElement("button");
    action.className = "job-action";
    if (job.status === "done" && job.output) {
      action.textContent = "📂";
      action.title = t("openDir");
      action.addEventListener("click", () => invoke("reveal", { path: job.output }));
    } else if (job.status === "queued" || job.status === "running") {
      action.textContent = "✕";
      action.title = t("cancelled");
      action.addEventListener("click", () => invoke("cancel_job", { id: job.id }));
    } else {
      action.textContent = "·";
      action.disabled = true;
    }
    card.appendChild(action);
    queueList.appendChild(card);
  }
  queueSection.classList.toggle("hidden", jobs.size === 0);
}

/* ---------------- history ---------------- */

interface HistoryEntry {
  id: number;
  source: string;
  output: string;
  target: string;
  when: string;
  archived?: boolean;
  archived_at?: string | null;
}

type HistoryView = "recent" | "archive";
let historyView: HistoryView = "recent";

async function renderHistory() {
  if (!isTauri) return;
  const items = await invoke<HistoryEntry[]>("get_history").catch(() => []);
  const archivedCount = items.filter((h) => h.archived).length;
  const list = items.filter((h) => (historyView === "archive" ? h.archived : !h.archived));
  histTabArchive.textContent =
    t("archive") + (archivedCount ? ` (${archivedCount})` : "");
  historyList.innerHTML = "";
  historySection.classList.toggle("hidden", items.length === 0);
  for (const h of list.slice(0, 30)) {
    const el = document.createElement("div");
    el.className = "history-item";
    const when = historyView === "archive" && h.archived_at ? h.archived_at : h.when;
    el.innerHTML = `
      <span class="h-target">${escapeHtml(h.target)}</span>
      <span class="h-path" title="${escapeHtml(h.output)}">${escapeHtml(h.output)}</span>
      <span class="h-when">${escapeHtml(when)}</span>`;
    el.addEventListener("click", () => invoke("reveal", { path: h.output }));

    const open = document.createElement("button");
    open.className = "ghost-btn small";
    open.textContent = t("openDir");
    open.addEventListener("click", (e) => {
      e.stopPropagation();
      invoke("reveal", { path: h.output });
    });
    el.appendChild(open);

    if (historyView === "recent") {
      const del = document.createElement("button");
      del.className = "ghost-btn small";
      del.textContent = t("archiveAction");
      del.addEventListener("click", async (e) => {
        e.stopPropagation();
        await invoke("archive_history", { id: h.id }).catch(() => {});
        renderHistory();
      });
      el.appendChild(del);
    } else {
      const restore = document.createElement("button");
      restore.className = "ghost-btn small";
      restore.textContent = t("restoreAction");
      restore.addEventListener("click", async (e) => {
        e.stopPropagation();
        await invoke("restore_history", { id: h.id }).catch(() => {});
        renderHistory();
      });
      el.appendChild(restore);

      // Two-click confirm: first click arms the button, second purges.
      const purge = document.createElement("button");
      purge.className = "ghost-btn small danger";
      purge.textContent = t("purgeAction");
      purge.addEventListener("click", async (e) => {
        e.stopPropagation();
        if (!purge.classList.contains("armed")) {
          purge.classList.add("armed");
          purge.textContent = t("purgeConfirm");
          setTimeout(() => {
            purge.classList.remove("armed");
            purge.textContent = t("purgeAction");
          }, 4000);
          return;
        }
        await invoke("purge_history", { id: h.id }).catch(() => {});
        renderHistory();
      });
      el.appendChild(purge);
    }
    historyList.appendChild(el);
  }
}

/* ---------------- misc ui ---------------- */

function status(msg: string) {
  statusText.textContent = msg;
}

async function refreshEngineStatus() {
  if (!isTauri) return;
  const st = await invoke<[string, boolean][]>("engines_status").catch(() => []);
  const missing = st.filter(([, ok]) => !ok).map(([n]) => n);
  engineStatus.classList.toggle("degraded", missing.length > 0);
  engineStatus.title = missing.length
    ? `${t("engineMissing")}: ${missing.join(", ")}`
    : t("engineOk");
}

function bindStatic() {
  $("#theme-toggle").addEventListener("click", () => {
    const light = document.documentElement.classList.toggle("light");
    localStorage.setItem("morpho-theme", light ? "light" : "dark");
  });
  if (localStorage.getItem("morpho-theme") === "light") {
    document.documentElement.classList.add("light");
  }

  const langBtn = $("#lang-toggle");
  langBtn.textContent = lang === "zh" ? "EN" : "中";
  langBtn.addEventListener("click", () => {
    setLang(lang === "zh" ? "en" : "zh");
    location.reload();
  });

  dropzone.addEventListener("click", async () => {
    if (!isTauri) { fileInput.click(); return; }
    const picked = await dialogOpen({ multiple: true });
    if (Array.isArray(picked)) addPaths(picked);
    else if (typeof picked === "string") addPaths([picked]);
  });
  fileInput.addEventListener("change", () => {
    // browser preview: no real paths available
    for (const f of Array.from(fileInput.files ?? [])) {
      const ext = extOf(f.name);
      if (!matrix.has(ext)) continue;
      files.push({ path: f.name, name: f.name, size: f.size, fmt: ext });
    }
    filesChanged();
    fileInput.value = "";
  });

  dropzone.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") dropzone.click();
  });

  qualitySlider.addEventListener("input", () => {
    qualityValue.textContent = qualitySlider.value;
  });

  convertBtn.addEventListener("click", convert);

  pickOutput.addEventListener("click", async () => {
    if (!isTauri) return;
    const dir = await dialogOpen({ directory: true });
    if (typeof dir === "string") {
      outputDir = dir;
      outputLabel.textContent = dir;
      clearOutput.classList.remove("hidden");
    }
  });
  clearOutput.addEventListener("click", () => {
    outputDir = null;
    outputLabel.textContent = t("outputSame");
    clearOutput.classList.add("hidden");
  });

  clearHistoryBtn.addEventListener("click", async () => {
    await invoke("clear_history").catch(() => {});
    renderHistory();
  });

  for (const [tab, view] of [
    [histTabRecent, "recent"],
    [histTabArchive, "archive"],
  ] as const) {
    tab.addEventListener("click", () => {
      historyView = view;
      histTabRecent.classList.toggle("active", view === "recent");
      histTabArchive.classList.toggle("active", view === "archive");
      renderHistory();
    });
  }
}

async function bindDragDrop() {
  if (!isTauri) return;
  await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      dropzone.classList.remove("dragover");
      addPaths(event.payload.paths);
    } else if (event.payload.type === "enter" || event.payload.type === "over") {
      dropzone.classList.add("dragover");
    }
  });
}

async function bindJobEvents() {
  if (!isTauri) return;
  await listen<Record<string, unknown>>("job-event", (ev) => {
    const p = ev.payload as any;
    const id = p.id as number;
    let job = jobs.get(id);
    if (!job) {
      if (p.type === "queued" || p.type === "started") {
        job = {
          id,
          name: p.name ? basename(p.name) : p.type,
          target: "…",
          ratio: 0,
          status: p.type === "queued" ? "queued" : "running",
          note: "",
        };
        jobs.set(id, job);
      } else {
        return;
      }
    }
    switch (p.type) {
      case "started":
        job.status = "running";
        break;
      case "progress":
        job.status = "running";
        job.ratio = p.ratio ?? 0;
        job.note = p.note ?? "";
        break;
      case "done":
        job.status = "done";
        job.ratio = 1;
        job.output = p.output;
        break;
      case "failed":
        job.status = "failed";
        job.error = p.error ?? "";
        break;
      case "cancelled":
        job.status = "cancelled";
        break;
    }
    renderQueue();
  });
}

/* ---------------- boot ---------------- */

async function boot() {
  applyStatic();
  bindStatic();

  const entries = await invoke<MatrixEntry[]>("get_matrix").catch(() => mockMatrix());
  for (const e of entries) matrix.set(e.format, e);

  if (!isTauri) {
    const notice = document.createElement("div");
    notice.className = "notice";
    notice.textContent = t("browserHint");
    $(".content").prepend(notice);
  }

  await Promise.all([bindDragDrop(), bindJobEvents(), renderHistory(), refreshEngineStatus()]);
  status(`🦋 Morpho v0.1.0 — ${t("localOnly")}`);
}

function mockMatrix(): MatrixEntry[] {
  const all = ["png","jpg","webp","gif","bmp","tiff","ico","mp4","mkv","webm","mov","mp3","wav","flac","txt","md","html","docx","pdf","xlsx","pptx"];
  return all.map((f) => ({ format: f, category: "image", targets: all }));
}

boot();
