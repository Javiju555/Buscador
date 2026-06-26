import "./style.css";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { tr } from "./i18n";

type SearchResultKind = "app" | "command" | "file" | "web" | "calculation" | "info";

interface SearchResult {
  kind: SearchResultKind;
  title: string;
  subtitle: string;
  primaryValue: string;
  score: number;
}

interface SearchResponse {
  results: SearchResult[];
  fileIndexing: boolean;
}

interface LauncherSettings {
  startWithWindows: boolean;
  roots: string[];
  maxFiles: number;
  webProvider: string;
  webApiKey: string;
  semanticRoots: string[];
}

const COLLAPSED_HEIGHT = 92;
const MAX_WINDOW_HEIGHT = 680;
const RESULTS_LIMIT = 6;
const SEARCH_LIMIT = 12;
const FULL_SEARCH_DELAY_MS = 130;
const ICON_CACHE_LIMIT = RESULTS_LIMIT * 2;
const INPUT_FOCUS_RETRIES = 5;
const INPUT_FOCUS_RETRY_DELAY_MS = 50;

const appElement = document.querySelector<HTMLDivElement>("#app");
if (!appElement) {
  throw new Error("No se encontro #app");
}
const appRoot: HTMLDivElement = appElement;

function blockBrowserZoom(): void {
  if (document.documentElement.dataset.zoomGuardInstalled === "1") return;
  document.documentElement.dataset.zoomGuardInstalled = "1";

  const preventIfCancelable = (event: Event): void => {
    if (event.cancelable) event.preventDefault();
  };

  window.addEventListener("wheel", event => {
    if (!event.ctrlKey && !event.metaKey) return;
    preventIfCancelable(event);
  }, { passive: false, capture: true });

  const gestureHandler = preventIfCancelable as EventListener;
  ["gesturestart", "gesturechange", "gestureend"].forEach(type => {
    document.addEventListener(type, gestureHandler, { passive: false, capture: true });
  });

  document.documentElement.style.touchAction = "pan-x pan-y";
  if (document.body) document.body.style.touchAction = "pan-x pan-y";
}

blockBrowserZoom();

appRoot.innerHTML = `
  <main class="launcher-shell">
    <section class="launcher-pill">
      <input
        id="query-input"
        class="query-input"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder="${tr('Buscar apps, comandos, archivos, web (w ...) o calculos')}"
      />
      <div class="pill-actions">
        <button id="settings-toggle" class="settings-toggle" type="button" title="${tr('Ajustes')}">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="3"/>
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
          </svg>
        </button>
      </div>
    </section>
    <section id="settings-panel" class="settings-panel hidden">
      <p class="settings-title">${tr('Ajustes')}</p>
      <label class="settings-label" for="settings-roots">${tr('Carpetas raiz (; separado)')}</label>
      <input
        id="settings-roots"
        class="settings-input"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder="${tr('D:\\Documentos;D:\\Proyectos')}"
      />
      <label class="settings-label" for="settings-max-files">${tr('Maximo de archivos')}</label>
      <input id="settings-max-files" class="settings-input" type="number" min="3000" max="100000" step="500" />
      <label class="settings-label" for="settings-semantic-roots">${tr('Carpetas para busqueda semantica (; separado)')}</label>
      <input
        id="settings-semantic-roots"
        class="settings-input"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder="${tr('~/Documentos;~/Notas')}"
      />
      <label class="settings-label" for="settings-web-provider">${tr('Proveedor web (opcional)')}</label>
      <input
        id="settings-web-provider"
        class="settings-input"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder="brave"
      />
      <label class="settings-label" for="settings-web-api-key">${tr('API key web (opcional)')}</label>
      <input
        id="settings-web-api-key"
        class="settings-input"
        type="password"
        autocomplete="off"
        spellcheck="false"
        placeholder="${tr('Si vacio: solo abrir busqueda en navegador')}"
      />
      <label class="settings-checkbox-row" for="settings-start-with-windows">
        <input id="settings-start-with-windows" type="checkbox" />
        <span>${tr('Iniciar con Windows')}</span>
      </label>
      <div class="settings-actions">
        <button id="settings-save" class="settings-button primary" type="button">${tr('Guardar y reindexar')}</button>
        <button id="settings-reindex" class="settings-button" type="button">${tr('Reindexar')}</button>
        <button id="settings-download-model" class="settings-button" type="button">${tr('Descargar modelo')}</button>
      </div>
      <p id="settings-status" class="settings-status"></p>
    </section>
    <section id="dropdown-panel" class="dropdown-panel hidden">
      <article id="calc-hero" class="calc-hero hidden">
        <p id="calc-expression" class="calc-expression"></p>
        <p id="calc-value" class="calc-value"></p>
      </article>
      <p id="status-line" class="status-line"></p>
      <div id="results-list" class="results-list"></div>
    </section>

  </main>
`;

const queryInput = document.querySelector<HTMLInputElement>("#query-input")!;
const dropdownPanel = document.querySelector<HTMLElement>("#dropdown-panel")!;
const calcHero = document.querySelector<HTMLElement>("#calc-hero")!;
const calcExpression = document.querySelector<HTMLElement>("#calc-expression")!;
const calcValue = document.querySelector<HTMLElement>("#calc-value")!;
const statusLine = document.querySelector<HTMLElement>("#status-line")!;
const resultsList = document.querySelector<HTMLElement>("#results-list")!;
const settingsToggle = document.querySelector<HTMLButtonElement>("#settings-toggle")!;
const settingsPanel = document.querySelector<HTMLElement>("#settings-panel")!;
const settingsRootsInput = document.querySelector<HTMLInputElement>("#settings-roots")!;
const settingsSemanticRootsInput = document.querySelector<HTMLInputElement>("#settings-semantic-roots")!;
const settingsMaxFilesInput = document.querySelector<HTMLInputElement>("#settings-max-files")!;
const settingsWebProviderInput = document.querySelector<HTMLInputElement>("#settings-web-provider")!;
const settingsWebApiKeyInput = document.querySelector<HTMLInputElement>("#settings-web-api-key")!;
const settingsStartWithWindowsInput = document.querySelector<HTMLInputElement>(
  "#settings-start-with-windows",
)!;
const settingsSaveButton = document.querySelector<HTMLButtonElement>("#settings-save")!;
const settingsReindexButton = document.querySelector<HTMLButtonElement>("#settings-reindex")!;
const settingsDownloadModelButton = document.querySelector<HTMLButtonElement>("#settings-download-model")!;
const settingsStatus = document.querySelector<HTMLElement>("#settings-status")!;
const launcherPill = document.querySelector<HTMLElement>(".launcher-pill")!;

let debounceTimer: number | undefined;
let currentQuery = "";
let selectedIndex = 0;
let lastResponse: SearchResponse = { results: [], fileIndexing: false };
let nonCalculationResults: SearchResult[] = [];
let calculationResult: SearchResult | undefined;
let resizeFrame: number | undefined;
let lastWindowHeight = 0;
let fullSearchTimer: number | undefined;
let activeSearchId = 0;
let progressivePhaseActive = false;
let settingsOpen = false;
let settingsLoaded = false;
let selectionMode: "calculation" | "results" = "results";

const iconCache = new Map<string, string | null>();
const pendingIcons = new Set<string>();

function setIconCache(key: string, value: string | null): void {
  iconCache.delete(key); // refresca posición si ya existe
  iconCache.set(key, value);
  if (iconCache.size > ICON_CACHE_LIMIT) {
    iconCache.delete(iconCache.keys().next().value!);
  }
}

initThemeSync();
initFocusEvent();
initWindowFocusGuards();
initInputHandlers();
initKeyboardHandlers();
initResultsHandlers();
initSettingsHandlers();

focusQueryInput();
scheduleResize();

function initThemeSync(): void {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const applyTheme = (theme: "dark" | "light"): void => {
    document.documentElement.dataset.theme = theme;
  };

  const sync = async (): Promise<void> => {
    try {
      const systemTheme = await invoke<string>("system_theme");
      if (systemTheme === "dark" || systemTheme === "light") {
        applyTheme(systemTheme);
        return;
      }
    } catch {
      // fallback below
    }
    applyTheme(media.matches ? "dark" : "light");
  };

  void sync();
  media.addEventListener("change", () => {
    void sync();
  });
}

function initFocusEvent(): void {
  listen("launcher-show", () => {
    void invoke<string>("system_theme")
      .then((theme) => {
        if (theme === "dark" || theme === "light") {
          document.documentElement.dataset.theme = theme;
        }
      })
      .catch(() => undefined);
    resetState();
    focusQueryInput();
    scheduleResize();
  }).catch(console.error);
}

function initWindowFocusGuards(): void {
  window.addEventListener("focus", () => {
    focusQueryInput(6);
  });

  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) {
      focusQueryInput(6);
    }
  });

  launcherPill.addEventListener("mousedown", () => {
    focusQueryInput(8);
  });
}

function initInputHandlers(): void {
  queryInput.addEventListener("input", () => {
    if (settingsOpen) {
      closeSettingsPanel();
    }
    currentQuery = queryInput.value;
    if (debounceTimer) {
      window.clearTimeout(debounceTimer);
    }
    debounceTimer = window.setTimeout(() => void runSearch(currentQuery), 75);
  });
}

function initSettingsHandlers(): void {
  settingsToggle.addEventListener("click", () => {
    if (settingsOpen) {
      closeSettingsPanel();
      queryInput.focus();
      return;
    }

    void openSettingsPanel();
  });

  settingsSaveButton.addEventListener("click", () => {
    void saveSettingsFromUI();
  });

  settingsReindexButton.addEventListener("click", async () => {
    try {
      settingsStatus.textContent = tr('Reindexando...');
      await invoke("reindex_files");
      settingsStatus.textContent = tr('Reindexado lanzado en segundo plano.');
    } catch (error) {
      settingsStatus.textContent = `${tr("Could not reindex")}: ${String(error)}`;
    }
  });

  settingsDownloadModelButton.addEventListener("click", async () => {
    try {
      settingsDownloadModelButton.disabled = true;
      settingsStatus.textContent = tr('Descargando modelo...');
      await invoke("download_embedding_model");
      settingsStatus.textContent = tr('Modelo descargado con exito.');
    } catch (error) {
      settingsStatus.textContent = `${tr("Error al descargar modelo")}: ${String(error)}`;
    } finally {
      settingsDownloadModelButton.disabled = false;
    }
  });
}

function initResultsHandlers(): void {
  resultsList.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) {
      return;
    }

    const row = target.closest<HTMLButtonElement>(".result-row");
    if (!row || !resultsList.contains(row)) {
      return;
    }

    const rowIndex = Number.parseInt(row.dataset.index ?? "", 10);
    if (!Number.isFinite(rowIndex) || rowIndex < 0 || rowIndex >= nonCalculationResults.length) {
      return;
    }

    selectResultIndex(rowIndex);
    void executeSelection();
  });
}

function initKeyboardHandlers(): void {
  window.addEventListener("keydown", async (event) => {
    if (event.key === "Escape" && settingsOpen) {
      event.preventDefault();
      closeSettingsPanel();
      queryInput.focus();
      return;
    }

    if (event.key === "," && event.ctrlKey) {
      event.preventDefault();
      if (settingsOpen) {
        closeSettingsPanel();
        queryInput.focus();
      } else {
        await openSettingsPanel();
      }
      return;
    }

    if (settingsOpen && event.key === "Enter") {
      if (document.activeElement === settingsSaveButton || document.activeElement === settingsReindexButton) {
        return;
      }
      event.preventDefault();
      await saveSettingsFromUI();
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      await invoke("hide_launcher");
      return;
    }

    if (event.key === "Tab") {
      const applied = applyMathAutocompleteFromSelection();
      if (applied) {
        event.preventDefault();
      }
      return;
    }

    if (event.key === "ArrowDown") {
      if (nonCalculationResults.length > 0) {
        event.preventDefault();
        if (selectionMode === "calculation") {
          selectionMode = "results";
          selectResultIndex(0);
        } else {
          const nextIndex = (selectedIndex + 1) % nonCalculationResults.length;
          selectResultIndex(nextIndex);
        }
        updateStatus(currentQuery);
        renderResults();
      }
      return;
    }

    if (event.key === "ArrowUp") {
      if (nonCalculationResults.length > 0) {
        event.preventDefault();
        if (selectionMode === "calculation") {
          selectResultIndex(nonCalculationResults.length - 1);
          selectionMode = "results";
        } else if (selectedIndex === 0 && calculationResult) {
          selectionMode = "calculation";
        } else {
          const nextIndex =
            (selectedIndex - 1 + nonCalculationResults.length) % nonCalculationResults.length;
          selectResultIndex(nextIndex);
        }
        updateStatus(currentQuery);
        renderResults();
      }
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      await executeSelection();
    }
  });
}

async function runSearch(query: string): Promise<void> {
  const trimmed = query.trim();
  if (!trimmed) {
    resetState();
    return;
  }

  const searchId = ++activeSearchId;
  progressivePhaseActive = shouldUseProgressivePhase(trimmed);
  selectedIndex = 0;

  try {
    const fastResponse = await invoke<SearchResponse>("search_fast", {
      query: trimmed,
      limit: SEARCH_LIMIT,
    });
    if (!isSearchCurrent(searchId, trimmed)) {
      return;
    }

    applyResponse(fastResponse, trimmed);

    if (!progressivePhaseActive) {
      return;
    }

    if (fullSearchTimer) {
      window.clearTimeout(fullSearchTimer);
    }

    fullSearchTimer = window.setTimeout(async () => {
      try {
        const fullResponse = await invoke<SearchResponse>("search", {
          query: trimmed,
          limit: SEARCH_LIMIT,
        });
        if (!isSearchCurrent(searchId, trimmed)) {
          return;
        }

        progressivePhaseActive = false;
        applyResponse(fullResponse, trimmed);
      } catch (error) {
        if (!isSearchCurrent(searchId, trimmed)) {
          return;
        }

        progressivePhaseActive = false;
statusLine.textContent = `${tr("Error in search")}: ${String(error)}`;
    }
      }, FULL_SEARCH_DELAY_MS);
  } catch (error) {
    progressivePhaseActive = false;
    statusLine.textContent = `${tr("Error in search")}: ${String(error)}`;
    dropdownPanel.classList.remove("hidden");
    scheduleResize();
  }
}

function applyResponse(response: SearchResponse, query: string): void {
  lastResponse = response;
  calculationResult = response.results.find((item) => item.kind === "calculation");
  nonCalculationResults = response.results
    .filter((item) => item.kind !== "calculation")
    .slice(0, RESULTS_LIMIT);

  if (calculationResult) {
    selectionMode = "calculation";
  } else {
    selectionMode = "results";
  }

  selectedIndex = Math.min(selectedIndex, Math.max(nonCalculationResults.length - 1, 0));
  updateStatus(query);
  renderPanel(query);
  renderResults();
  scheduleResize();
}

function isSearchCurrent(searchId: number, query: string): boolean {
  return activeSearchId === searchId && currentQuery.trim() === query.trim();
}

function shouldUseProgressivePhase(query: string): boolean {
  const trimmed = query.trim();
  if (!trimmed) {
    return false;
  }
  return (
    !trimmed.startsWith(">") &&
    !trimmed.startsWith("=") &&
    !trimmed.startsWith("w ") &&
    !trimmed.startsWith("w:")
  );
}

async function openSettingsPanel(): Promise<void> {
  settingsOpen = true;
  settingsToggle.classList.add("active");
  settingsPanel.classList.remove("hidden");
  dropdownPanel.classList.add("hidden");
  statusLine.textContent = "";

  if (!settingsLoaded) {
    await loadSettingsIntoUI();
  }

  scheduleResize();
  settingsRootsInput.focus();
}

function closeSettingsPanel(): void {
  settingsOpen = false;
  settingsToggle.classList.remove("active");
  settingsPanel.classList.add("hidden");
  settingsStatus.textContent = "";
  focusQueryInput();
  scheduleResize();
}

async function loadSettingsIntoUI(): Promise<void> {
  try {
    const settings = await invoke<LauncherSettings>("get_settings");
    settingsRootsInput.value = settings.roots.join(";");
    settingsSemanticRootsInput.value = (settings.semanticRoots ?? []).join(";");
    settingsMaxFilesInput.value = String(settings.maxFiles);
    settingsWebProviderInput.value = settings.webProvider ?? "";
    settingsWebApiKeyInput.value = settings.webApiKey ?? "";
    settingsStartWithWindowsInput.checked = settings.startWithWindows;
    settingsLoaded = true;
  } catch (error) {
    settingsStatus.textContent = `${tr("Could not load settings")}: ${String(error)}`;
  }
}

async function saveSettingsFromUI(): Promise<void> {
  const roots = settingsRootsInput.value
    .split(";")
    .map((value) => value.trim())
    .filter((value) => value.length > 0);
  const semanticRoots = settingsSemanticRootsInput.value
    .split(";")
    .map((value) => value.trim())
    .filter((value) => value.length > 0);
  const parsedMax = Number.parseInt(settingsMaxFilesInput.value, 10);
  const maxFiles = Number.isFinite(parsedMax) ? parsedMax : 25_000;
  const webProvider = settingsWebProviderInput.value.trim();
  const webApiKey = settingsWebApiKeyInput.value.trim();
  const startWithWindows = settingsStartWithWindowsInput.checked;

  try {
    settingsSaveButton.disabled = true;
    settingsStatus.textContent = tr("Guardando y reindexando...");
    const saved = await invoke<LauncherSettings>("save_settings", {
      settings: { startWithWindows, roots, maxFiles, webProvider, webApiKey, semanticRoots },
    });

    settingsRootsInput.value = saved.roots.join(";");
    settingsSemanticRootsInput.value = (saved.semanticRoots ?? []).join(";");
    settingsMaxFilesInput.value = String(saved.maxFiles);
    settingsWebProviderInput.value = saved.webProvider ?? "";
    settingsWebApiKeyInput.value = saved.webApiKey ?? "";
    settingsStartWithWindowsInput.checked = saved.startWithWindows;
    settingsStatus.textContent = tr("Ajustes guardados y reindexado lanzado.");
    settingsLoaded = true;
  } catch (error) {
    settingsStatus.textContent = `${tr("Could not save settings")}: ${String(error)}`;
  } finally {
    settingsSaveButton.disabled = false;
  }
}

function renderPanel(query: string): void {
  if (settingsOpen) {
    closeSettingsPanel();
  }
  dropdownPanel.classList.remove("hidden");

  if (calculationResult) {
    calcExpression.textContent = query.startsWith("=") ? query.slice(1).trim() : query;
    calcValue.textContent = calculationResult.primaryValue;
    calcHero.classList.remove("hidden");
  } else {
    calcHero.classList.add("hidden");
    calcExpression.textContent = "";
    calcValue.textContent = "";
  }

  scheduleResize();
}

function renderResults(): void {
  resultsList.innerHTML = "";
  for (let i = 0; i < nonCalculationResults.length; i += 1) {
    const result = nonCalculationResults[i];
    const row = document.createElement("button");
    row.type = "button";
    row.className = `result-row ${selectionMode === "results" && selectedIndex === i ? "selected" : ""}`;
    row.dataset.index = String(i);

    const iconSlot = document.createElement("span");
    iconSlot.className = "result-icon";
    renderResultIcon(iconSlot, result);

    const texts = document.createElement("span");
    texts.className = "result-texts";

    const title = document.createElement("span");
    title.className = "result-title";
    title.textContent = result.title;

    const subtitle = document.createElement("span");
    subtitle.className = "result-subtitle";
    subtitle.textContent = translateSubtitle(result.subtitle);

    texts.append(title, subtitle);

    const badge = document.createElement("span");
    badge.className = "result-badge";
    badge.textContent = badgeFor(result.kind);

    row.append(iconSlot, texts, badge);
    resultsList.appendChild(row);
  }

  scheduleResize();
}

function translateSubtitle(subtitle: string): string {
  const translated = tr(subtitle);
  return translated !== subtitle ? translated : subtitle;
}

function selectResultIndex(nextIndex: number): void {
  if (nonCalculationResults.length === 0) {
    selectedIndex = 0;
    return;
  }

  const boundedIndex = clamp(nextIndex, 0, nonCalculationResults.length - 1);
  if (boundedIndex === selectedIndex) {
    return;
  }

  const previousRow = getResultRow(selectedIndex);
  const nextRow = getResultRow(boundedIndex);

  selectedIndex = boundedIndex;
  previousRow?.classList.remove("selected");
  nextRow?.classList.add("selected");
}

function getResultRow(index: number): HTMLButtonElement | undefined {
  const row = resultsList.children.item(index);
  return row instanceof HTMLButtonElement ? row : undefined;
}

function updateStatus(query: string): void {
  if (query.trim().length === 0) {
    statusLine.textContent = tr('Escribe para buscar.');
    return;
  }

  if (nonCalculationResults.length === 0 && !calculationResult) {
    if (progressivePhaseActive) {
      statusLine.textContent = tr('Buscando tambien en archivos...');
    } else {
      statusLine.textContent = lastResponse.fileIndexing
        ? tr('Sin coincidencias por ahora. El indexado de archivos sigue en progreso.')
        : tr('Sin coincidencias.');
    }
    return;
  }

  if (nonCalculationResults.length === 0 && calculationResult) {
    statusLine.textContent = progressivePhaseActive
      ? tr("Enter para copiar resultado. Buscando tambien en archivos...")
      : tr("Enter para copiar el resultado de la calculadora.");
    return;
  }

  if (calculationResult && selectionMode === "calculation") {
    statusLine.textContent = progressivePhaseActive
      ? `${nonCalculationResults.length}${tr(" result(s). Enter copies calculation; ↓ to open results.")}`
      : `${nonCalculationResults.length}${tr(" result(s). Enter copies calculation; ↓ to open results.")}`;
    return;
  }

  const selected = nonCalculationResults[selectedIndex];
  if (selected && isMathAutocompleteResult(selected)) {
    statusLine.textContent = tr("Tab autocompleta formula · Enter tambien aplica sugerencia.");
    return;
  }

  statusLine.textContent = progressivePhaseActive
    ? `${nonCalculationResults.length}${tr(" result(s). Refining files...")}`
    : `${nonCalculationResults.length}${tr(" result(s). Enter to open.")}`;
}

async function executeSelection(): Promise<void> {
  if (selectionMode === "calculation" && calculationResult) {
    await invoke("copy_text", { text: calculationResult.primaryValue });
    statusLine.textContent = `${tr("Copied:")} ${calculationResult.primaryValue}`;
    return;
  }

  const selected = nonCalculationResults[selectedIndex];
  if (selected) {
    if (isMathAutocompleteResult(selected)) {
      applyMathAutocompleteFromSelection(selected);
      return;
    }

    await invoke("execute", {
      payload: {
        kind: selected.kind,
        title: selected.title,
        primaryValue: selected.primaryValue,
        rawQuery: currentQuery,
      },
    });

    await invoke("hide_launcher");
    return;
  }

  if (calculationResult) {
    await invoke("copy_text", { text: calculationResult.primaryValue });
    statusLine.textContent = `${tr("Copied:")} ${calculationResult.primaryValue}`;
  }
}

function isMathAutocompleteResult(result: SearchResult): boolean {
  return result.kind === "info" && result.primaryValue.startsWith("math_complete:");
}

function applyMathAutocompleteFromSelection(forcedResult?: SearchResult): boolean {
  const selected = forcedResult ?? nonCalculationResults[selectedIndex];
  if (!selected || !isMathAutocompleteResult(selected)) {
    return false;
  }

  const completion = selected.primaryValue.slice("math_complete:".length);
  if (!completion) {
    return false;
  }

  const { nextQuery, caretPosition } = applyMathCompletion(currentQuery, completion);
  queryInput.value = nextQuery;
  currentQuery = nextQuery;
  queryInput.focus({ preventScroll: true });
  queryInput.setSelectionRange(caretPosition, caretPosition);
  void runSearch(nextQuery);
  return true;
}

function applyMathCompletion(rawQuery: string, completion: string): { nextQuery: string; caretPosition: number } {
  const trimmedRight = rawQuery.replace(/\s+$/g, "");
  let start = trimmedRight.length;
  while (start > 0) {
    const previous = trimmedRight[start - 1];
    if (/[a-zA-Z_]/.test(previous)) {
      start -= 1;
      continue;
    }
    break;
  }

  const nextQuery = `${trimmedRight.slice(0, start)}${completion}`;
  const insideParentheses = completion.includes("(") && completion.endsWith(")");
  return {
    nextQuery,
    caretPosition: insideParentheses ? nextQuery.length - 1 : nextQuery.length,
  };
}

function resetState(): void {
  activeSearchId += 1;
  progressivePhaseActive = false;
  if (fullSearchTimer) {
    window.clearTimeout(fullSearchTimer);
    fullSearchTimer = undefined;
  }

  currentQuery = "";
  queryInput.value = "";
  selectedIndex = 0;
  selectionMode = "results";
  lastResponse = { results: [], fileIndexing: false };
  nonCalculationResults = [];
  calculationResult = undefined;
  dropdownPanel.classList.add("hidden");
  closeSettingsPanel();
  calcHero.classList.add("hidden");
  resultsList.innerHTML = "";
  statusLine.textContent = "";
  focusQueryInput();
  scheduleResize();
}

function focusQueryInput(maxRetries = INPUT_FOCUS_RETRIES): void {
  if (settingsOpen) {
    return;
  }

  let retries = 0;
  const attemptFocus = (): void => {
    if (settingsOpen) {
      return;
    }

    queryInput.focus({ preventScroll: true });
    const active = document.activeElement === queryInput;
    if (active && document.hasFocus()) {
      return;
    }

    retries += 1;
    if (retries < maxRetries) {
      window.setTimeout(attemptFocus, INPUT_FOCUS_RETRY_DELAY_MS);
    }
  };

  attemptFocus();
}

function renderResultIcon(slot: HTMLElement, result: SearchResult): void {
  const fallbackGlyph = iconForResult(result);
  const iconPath = iconPathFor(result);

  if (!iconPath) {
    slot.textContent = fallbackGlyph;
    return;
  }

  const cached = iconCache.get(iconPath);
  if (typeof cached === "string" && cached.length > 0) {
    const image = document.createElement("img");
    image.className = "result-icon-image";
    image.alt = "";
    image.src = cached;
    slot.replaceChildren(image);
    return;
  }

  slot.textContent = fallbackGlyph;
  if (cached === undefined) {
    requestIcon(iconPath);
  }
}

function requestIcon(path: string): void {
  if (pendingIcons.has(path) || iconCache.has(path)) {
    return;
  }

  pendingIcons.add(path);
  void invoke<string | null>("resolve_icon", { path })
    .then((iconData) => {
      setIconCache(path, iconData ?? null);
      renderResults();
    })
    .catch(() => {
      setIconCache(path, null);
    })
    .finally(() => {
      pendingIcons.delete(path);
    });
}

function iconPathFor(result: SearchResult): string | null {
  if (result.kind === "app") {
    const desktopEntryPath = result.subtitle.trim();
    if (desktopEntryPath.toLowerCase().endsWith(".desktop")) {
      return desktopEntryPath;
    }
    return result.primaryValue;
  }

  if (result.kind === "command" || result.kind === "file") {
    return result.primaryValue;
  }
  return null;
}

function scheduleResize(): void {
  if (resizeFrame !== undefined) {
    window.cancelAnimationFrame(resizeFrame);
  }

  resizeFrame = window.requestAnimationFrame(() => {
    resizeFrame = undefined;

    const targetHeight = computeTargetHeight();
    if (Math.abs(targetHeight - lastWindowHeight) < 2) {
      return;
    }

    lastWindowHeight = targetHeight;
    void invoke("resize_launcher", { height: targetHeight }).catch(() => undefined);
  });
}

function computeTargetHeight(): number {
  const dropdownHidden = dropdownPanel.classList.contains("hidden");
  const settingsHidden = settingsPanel.classList.contains("hidden");
  if (dropdownHidden && settingsHidden) {
    return COLLAPSED_HEIGHT;
  }

  return clamp(Math.ceil(appRoot.scrollHeight) + 4, COLLAPSED_HEIGHT, MAX_WINDOW_HEIGHT);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function iconFor(kind: SearchResultKind): string {
  switch (kind) {
    case "app":
      return "◉";
    case "command":
      return "⌘";
    case "file":
      return "▣";
    case "web":
      return "↗";
    case "calculation":
      return "∑";
    default:
      return "i";
  }
}

function iconForResult(result: SearchResult): string {
  if (result.kind !== "app") {
    return iconFor(result.kind);
  }

  for (const character of result.title.trim()) {
    const isNumber = character >= "0" && character <= "9";
    const isLetter = character.toLowerCase() !== character.toUpperCase();
    if (isNumber || isLetter) {
      return character.toUpperCase();
    }
  }

  return iconFor(result.kind);
}

function badgeFor(kind: SearchResultKind): string {
  switch (kind) {
    case "app":
      return "APP";
    case "command":
      return "CMD";
    case "file":
      return "FILE";
    case "web":
      return "WEB";
    case "calculation":
      return "CALC";
    default:
      return "INFO";
  }
}

