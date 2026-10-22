/* DIA has its own configuration, event stream and lazy, bounded result viewer.
 * The numerical engine is independent of DDA; only native dialogs and theme
 * are shared. No scan arrays or whole-run result tables live in the webview. */
import {
  BASELINES,
  baselineLabel,
  drawnWindow,
  moveEdge,
  nudgeEdge,
  previewBaseline,
  sameWindow,
  sequenceAxis,
  sequencePlan,
} from "./dia-integration.js";
const FIELDS = [
  ["ms1Tol", "MS1 tolerance (Da)", 0.001],
  ["ms2Tol", "MS2 tolerance (Da)", 0.01],
  ["ms2Score", "Minimum MS2 score", 0.05],
  ["minPeaks", "Minimum matched fragments", 1],
  ["rtShift", "RT shift (min)", 0.01],
  ["peakShape", "Peak shape", 0.05],
  ["snScore", "Smoothness threshold", 0.05],
  ["integrationRt", "Integration RT multiplier", 0.1],
  ["integrationMz", "Integration m/z (Da)", 0.001],
  ["imputeWidth", "Imputation half-width (min; 0 = auto)", 0.01],
  ["threads", "CPU workers (0 = auto)", 1],
  ["maxFilesInFlight", "Resident samples", 1],
];

export function parseWindows(text) {
  const values = text.trim().split(/[\s,;]+/).filter(Boolean).map(Number);
  if (
    !values.length || values.length % 2 ||
    values.some((v) => !Number.isFinite(v))
  ) {
    throw new Error("Isolation windows need complete lower/upper m/z pairs.");
  }
  const windows = [];
  for (let i = 0; i < values.length; i += 2) {
    if (values[i] < 0 || values[i] >= values[i + 1]) {
      throw new Error(
        "Each isolation window must have a lower m/z below its upper m/z.",
      );
    }
    windows.push([values[i], values[i + 1]]);
  }
  return windows;
}

export function createDia(
  { invoke, listen, dialogOpen, dialogSave, asPath, openOutputDirectory },
) {
  const $ = (id) => document.getElementById(`dia-${id}`);
  const root = $("shell");
  // Static application markup only. Dataset names and paths use textContent.
  root.innerHTML = `
    <nav class="rail" aria-label="DIA workflow">
      <header class="masthead"><h1 class="wordmark">MetaboKit</h1><p class="mono micro muted"><button class="mode-switch" data-acquisition="dda" title="Switch to DDA" aria-label="DIA mode. Switch to DDA">DIA</button><span data-app-version></span></p></header>
      <ul class="nav" role="tablist" aria-label="DIA pages">
        <li><button class="dia-nav-item is-active" data-dia-view="dataset" role="tab" aria-selected="true" aria-controls="dia-dataset"><span class="mono ord">01</span> Dataset</button></li>
        <li><button class="dia-nav-item" data-dia-view="run" role="tab" aria-selected="false" aria-controls="dia-run"><span class="mono ord">02</span> Run</button></li>
        <li><button class="dia-nav-item" data-dia-view="visualizer" role="tab" aria-selected="false" aria-controls="dia-visualizer"><span class="mono ord">03</span> Visualizer</button></li>
      </ul>
      <div class="rail-foot">
        <p class="mono micro muted" id="dia-status" role="status">No dataset</p>
        <div class="meter" aria-hidden="true"><span id="dia-meter"></span></div>
        <button class="btn btn-primary" id="dia-start" disabled>Run DIA analysis</button>
        <button class="btn btn-quiet is-hidden" id="dia-stop">Stop</button>
        <div class="rail-links"><button class="linky mono micro" id="dia-theme-toggle" type="button" aria-label="Toggle light and dark appearance">Appearance</button></div>
      </div>
    </nav>
    <main class="stage" id="dia-stage">
      <section class="dia-view" id="dia-dataset" role="tabpanel">
        <header class="sec-head"><p class="mono kicker">01 / Dataset</p><div class="sec-body"><h2>Choose a DIA folder</h2><p class="lede">Centroided mzML, an RT-annotated spectral library, and your acquisition's isolation windows. An original param.txt is imported automatically.</p></div></header>
        <div class="dia-toolbar"><button class="btn btn-primary" id="dia-choose">Choose folder…</button><button class="btn" id="dia-load">Import param.txt / preset…</button><button class="btn btn-quiet" id="dia-save" disabled>Save preset…</button></div>
        <p class="mono dia-path" id="dia-dataset-path"></p><p class="muted" id="dia-notes"></p>
        <div id="dia-config" class="is-hidden">
          <details class="dia-details"><summary id="dia-file-count">Samples</summary><ul class="dia-files" id="dia-files"></ul></details>
          <div class="dia-field"><label for="dia-output">Output folder</label><div class="dia-inline"><input type="text" id="dia-output" spellcheck="false"><button class="btn" id="dia-output-choose">Choose…</button></div><p class="muted">Use a new or empty folder. Existing results are never overwritten.</p></div>
          <div class="dia-field"><label for="dia-libraries">Libraries</label><textarea id="dia-libraries" rows="3" spellcheck="false" aria-describedby="dia-library-help"></textarea><div class="dia-toolbar"><button class="btn btn-quiet" id="dia-add-library">Add spectral library…</button><button class="btn btn-quiet" id="dia-add-targets">Add target CSV…</button></div><p class="muted" id="dia-library-help">One source per line: user /path/to/library.txt, csv /path/to/targets.csv, or a built-in library name. DIA identification requires retention times and MS2 fragments.</p></div>
          <div class="dia-field"><label for="dia-libs-dir">Metadata / built-in library folder (optional)</label><div class="dia-inline"><input type="text" id="dia-libs-dir" spellcheck="false"><button class="btn" id="dia-libs-choose">Choose…</button></div></div>
          <div class="dia-field"><label for="dia-positive">Polarity</label><select id="dia-positive"><option value="true">Positive</option><option value="false">Negative</option></select></div>
          <details class="dia-details" id="dia-parameters"><summary>Analysis parameters & resources</summary>
            <p class="muted">The original DIA thresholds and integration rules are retained. One resident sample limits scan memory; CPU workers are used within feature detection.</p>
            <div class="dia-fields" id="dia-fields"></div>
            <div class="dia-fields"><div class="dia-field"><label for="dia-width-min">Minimum peak width (min)</label><input type="number" id="dia-width-min" step="0.01"></div><div class="dia-field"><label for="dia-width-max">Maximum peak width (min)</label><input type="number" id="dia-width-max" step="0.01"></div></div>
            <div class="dia-field"><label for="dia-windows">Isolation windows (lower m/z, upper m/z)</label><textarea class="mono" id="dia-windows" rows="7" spellcheck="false"></textarea><p class="muted">Use the exact acquisition boundaries, one pair per line. Overlapping windows are supported.</p></div>
            <label class="dia-check"><input type="checkbox" id="dia-keep-cache"> Keep scan caches for the visualizer</label><p class="muted">Uncheck to remove generated scan caches after a successful run. Reports and original mzML files are kept.</p>
          </details>
        </div>
        <p class="dia-validation" id="dia-validation" role="status">Choose a dataset or open existing results in Visualizer.</p>
      </section>
      <section class="dia-view is-hidden" id="dia-run" role="tabpanel">
        <header class="sec-head"><p class="mono kicker">02 / Run</p><div class="sec-body"><h2 id="dia-run-title">Ready when you are</h2><p class="lede" id="dia-run-summary">DDA and DIA share one analysis slot to avoid competing for CPU and memory.</p></div></header>
        <div class="dia-toolbar"><button class="btn is-hidden" id="dia-results-folder">Open results folder</button><button class="btn is-hidden" id="dia-view-results">View chromatograms</button><button class="btn btn-quiet" id="dia-clear-log">Clear log</button></div>
        <ol class="log" id="dia-log" aria-label="DIA run log"></ol>
      </section>
      <section class="dia-view is-hidden" id="dia-visualizer" role="tabpanel">
        <header class="sec-head"><p class="mono kicker">03 / Visualizer</p><div class="sec-body"><h2>Chromatograms &amp; integration</h2><p class="lede">Inspect precursor and fragment traces, adjust integration windows and export re-integrated areas. The engine's reports are never modified.</p></div></header>
        <div class="dia-toolbar"><button class="btn" id="dia-open-results">Open DIA results…</button><span class="mono dia-path" id="dia-result-path"></span></div>
        <div class="dia-export is-hidden" id="dia-export" role="group" aria-label="Integrated report">
          <div class="dia-export-text"><p class="mono micro dia-label">Integration</p><p id="dia-export-summary"></p></div>
          <div class="dia-export-actions"><label class="dia-select" for="dia-project-baseline"><span>Default baseline</span><select id="dia-project-baseline"></select></label><button class="btn" id="dia-export-run" type="button">Export integrated report</button><button class="btn btn-quiet is-hidden" id="dia-export-open" type="button">Show files</button></div>
        </div>
        <p class="muted" id="dia-view-status" role="status">Choose a result folder from this app or the original DIA engine.</p>
        <div class="dia-browser is-hidden" id="dia-browser">
          <aside class="dia-targets"><label for="dia-search">Search targets</label><input id="dia-search" type="text" placeholder="Name…" autocomplete="off"><div id="dia-target-list" role="group" aria-label="Targets"></div><div class="dia-toolbar"><button class="btn btn-quiet" id="dia-prev">Previous</button><span class="mono micro" id="dia-page"></span><button class="btn btn-quiet" id="dia-next">Next</button></div></aside>
          <div class="dia-traces">
            <div class="dia-fields"><div class="dia-field"><label for="dia-sample">Sample</label><select id="dia-sample"></select></div><div class="dia-field"><label for="dia-trace-width">RT half-width (min)</label><input type="number" id="dia-trace-width" min="0.05" max="5" step="0.05" value="0.3"></div></div>
            <h3 id="dia-target-title"></h3><p class="mono micro" id="dia-target-metrics"></p>
            <section class="dia-integration is-hidden" id="dia-integration" aria-label="Integration window">
              <div class="dia-int-head">
                <p class="dia-int-window"><span class="mono micro dia-label">Window</span><span class="mono" id="dia-int-range"></span><span class="mono micro dia-int-source" id="dia-int-source"></span></p>
                <div class="dia-int-controls"><label class="dia-select" for="dia-int-baseline"><span>Baseline</span><select id="dia-int-baseline"></select></label><button class="btn btn-quiet" id="dia-int-reset" type="button">Restore engine window</button></div>
              </div>
              <details class="dia-seq" id="dia-seq-wrap" open>
                <summary><span id="dia-seq-summary">Acquisition sequence</span></summary>
                <div class="dia-seq-chart" id="dia-seq"></div>
                <div class="dia-int-tools"><button class="btn" id="dia-int-ref" type="button" aria-pressed="false">Use as reference</button><button class="btn" id="dia-int-preview" type="button">Preview interpolation</button><button class="btn btn-primary" id="dia-int-apply" type="button" disabled>Apply</button><button class="btn btn-quiet" id="dia-int-reset-all" type="button">Reset all samples</button></div>
                <p class="muted dia-int-hint" id="dia-int-hint" role="status"></p>
              </details>
              <p class="muted dia-int-help">Drag a window edge, or drag across a chromatogram to draw a new window. Edge keys: ← → move one scan, Shift moves five. Edits save automatically.</p>
            </section>
            <div id="dia-charts"></div>
          </div>
        </div>
      </section>
    </main>`;
  for (const [key, label, step] of FIELDS) {
    const field = document.createElement("div");
    field.className = "dia-field";
    const caption = document.createElement("label");
    caption.htmlFor = `dia-${key}`;
    caption.textContent = label;
    const input = document.createElement("input");
    Object.assign(input, {
      id: `dia-${key}`,
      type: "number",
      step: String(step),
    });
    input.min = "0";
    field.append(caption, input);
    $("fields").append(field);
  }

  let settings = null, files = [], running = false, validationToken = 0;
  let resultPath = "", session = null, offset = 0, selected = null;
  let viewerToken = 0,
    detailToken = 0,
    detailBusy = false,
    detailQueued = false,
    detailData = null;
  let restored = false, validateTimer, searchTimer;
  // Integration: per-target windows, the run's acquisition axis, references
  // chosen per target, an unsaved interpolation plan and an edit in progress.
  let overview = null, targetWindows = null, windowsToken = 0, plan = null;
  let axis = { timed: false, positions: [] };
  let pending = null, drag = null, charts = [];
  let paintFrame = 0, nudgeTimer = 0, resetArmed = 0;
  const references = new Map();
  for (const [value, label] of BASELINES) {
    $("project-baseline").append(new Option(label, value));
    $("int-baseline").append(new Option(label, value));
  }
  $("int-baseline").prepend(new Option("Project default", ""));
  try {
    if (localStorage.getItem("mk-dia-sequence") === "closed") {
      $("seq-wrap").open = false;
    }
  } catch { /* optional */ }
  const basename = (p) => String(p).split(/[\\/]/).pop();
  const fixed = (n, digits) => Number.isFinite(n) ? n.toFixed(digits) : "—";
  const store = (key, value) => {
    try {
      localStorage.setItem(key, value);
    } catch { /* optional */ }
  };
  const log = (message) => {
    const li = document.createElement("li");
    li.textContent = message;
    $("log").append(li);
    while ($("log").children.length > 500) $("log").firstChild.remove();
    $("log").scrollTop = $("log").scrollHeight;
  };
  const action = (fn) => async () => {
    try {
      await fn();
    } catch (err) {
      $("validation").textContent = String(err);
      log(String(err));
    }
  };
  function showView(name) {
    root.querySelectorAll(".dia-view").forEach((v) =>
      v.classList.toggle("is-hidden", v.id !== `dia-${name}`)
    );
    root.querySelectorAll("[data-dia-view]").forEach((b) => {
      const active = b.dataset.diaView === name;
      b.classList.toggle("is-active", active);
      b.setAttribute("aria-selected", String(active));
    });
    $("stage").scrollTop = 0;
    if (name === "visualizer") redraw();
  }
  function collect() {
    if (!settings) throw new Error("Choose a DIA dataset first.");
    const next = { ...settings };
    for (const [key] of FIELDS) {
      next[key] = $(key).valueAsNumber;
      if (!Number.isFinite(next[key])) {
        throw new Error(`Enter a valid number for ${key}.`);
      }
    }
    next.mzmlFiles = files.filter((_, i) =>
      $("files").children[i].querySelector("input").checked
    );
    next.outputDir = $("output").value.trim();
    next.libraries = $("libraries").value.split("\n").map((s) => s.trim())
      .filter(Boolean);
    next.libsDir = $("libs-dir").value.trim() || null;
    next.positive = $("positive").value === "true";
    next.peakWidth = [
      $("width-min").valueAsNumber,
      $("width-max").valueAsNumber,
    ];
    next.windows = parseWindows($("windows").value);
    next.keepCache = $("keep-cache").checked;
    return next;
  }
  async function validate() {
    const token = ++validationToken;
    $("start").disabled = true;
    try {
      const next = collect();
      if (!invoke) {
        throw new Error("The native app is required to run an analysis.");
      }
      await invoke("dia_validate", { settings: next });
      if (token !== validationToken) return false;
      $("validation").textContent =
        `${next.mzmlFiles.length} samples · ${next.windows.length} isolation windows · ready`;
      $("start").disabled = running;
      return true;
    } catch (err) {
      if (token === validationToken) $("validation").textContent = String(err);
      return false;
    }
  }
  function render(next) {
    settings = next;
    files = [...next.mzmlFiles];
    $("config").classList.remove("is-hidden");
    $("save").disabled = false;
    $("files").replaceChildren();
    for (const file of files) {
      const li = document.createElement("li"),
        label = document.createElement("label"),
        check = document.createElement("input");
      check.type = "checkbox";
      check.checked = true;
      label.title = file;
      label.append(check, document.createTextNode(basename(file)));
      li.append(label);
      $("files").append(li);
    }
    $("file-count").textContent =
      `${files.length} samples · click to review selection`;
    $("output").value = next.outputDir;
    $("libraries").value = next.libraries.join("\n");
    $("libs-dir").value = next.libsDir || "";
    $("positive").value = String(next.positive);
    for (const [key] of FIELDS) $(key).value = next[key];
    $("width-min").value = next.peakWidth[0];
    $("width-max").value = next.peakWidth[1];
    $("windows").value = next.windows.map((w) => w.join(", ")).join("\n");
    $("keep-cache").checked = next.keepCache;
    $("status").textContent = `${files.length} samples`;
    if (!next.windows.length) $("parameters").open = true;
    validate();
  }
  async function scan(path) {
    $("status").textContent = "Reading dataset…";
    const data = await invoke("dia_scan", { path });
    render(data.settings);
    $("dataset-path").textContent = path;
    $("notes").textContent = `${
      (data.totalBytes / 1024 ** 2).toFixed(1)
    } MiB input. ${data.notes.join(" ")}`;
    store("mk-dia-dataset", path);
  }
  function busy(on) {
    running = on;
    root.querySelectorAll("input, textarea, select, button").forEach((e) => {
      e.disabled = on;
    });
    root.querySelectorAll("[data-dia-view]").forEach((e) => {
      e.disabled = false;
    });
    document.querySelectorAll("[data-acquisition]").forEach((e) => {
      e.disabled = on;
    });
    $("stop").disabled = false;
    $("stop").classList.toggle("is-hidden", !on);
    $("start").classList.toggle("is-hidden", on);
    if (!on) {
      validate();
      updatePaging();
      refreshControls();
    }
  }
  function finish(title, message) {
    busy(false);
    $("status").textContent = title;
    $("run-title").textContent = title;
    $("run-summary").textContent = message;
    log(message);
  }
  $("choose").onclick = action(async () => {
    const path = asPath(
      await dialogOpen({ directory: true, title: "Choose DIA dataset" }),
    );
    if (path) await scan(path);
  });
  $("load").onclick = action(async () => {
    const path = asPath(
      await dialogOpen({
        filters: [{ name: "DIA parameters", extensions: ["txt", "json"] }],
      }),
    );
    if (path) {
      render(await invoke("dia_load_preset", { path }));
      $("dataset-path").textContent = path;
      $("notes").textContent =
        "Imported DIA configuration. Review paths before running.";
    }
  });
  $("save").onclick = action(async () => {
    const next = collect();
    const path = asPath(
      await dialogSave({
        defaultPath: "dia-settings.json",
        filters: [{ name: "DIA preset", extensions: ["json"] }],
      }),
    );
    if (path) await invoke("dia_save_preset", { path, settings: next });
  });
  for (
    const [button, input] of [["output-choose", "output"], [
      "libs-choose",
      "libs-dir",
    ]]
  ) {
    $(button).onclick = action(async () => {
      const path = asPath(await dialogOpen({ directory: true }));
      if (path) {
        $(input).value = path;
        validate();
      }
    });
  }
  for (
    const [button, prefix, extensions] of [["add-library", "user", [
      "txt",
      "msp",
    ]], ["add-targets", "csv", ["csv"]]]
  ) {
    $(button).onclick = action(async () => {
      const paths = await dialogOpen({
        multiple: true,
        filters: [{ name: "DIA library", extensions }],
      });
      for (const item of (Array.isArray(paths) ? paths : [paths])) {
        const path = asPath(item);
        if (path) {
          $("libraries").value += `${
            $("libraries").value ? "\n" : ""
          }${prefix} ${path}`;
        }
      }
      validate();
    });
  }
  $("dataset").addEventListener("input", () => {
    clearTimeout(validateTimer);
    validateTimer = setTimeout(validate, 150);
  });
  $("start").onclick = action(async () => {
    if (!(await validate())) {
      showView("dataset");
      return;
    }
    const next = collect();
    detailToken++;
    detailData = null;
    $("results-folder").classList.add("is-hidden");
    $("view-results").classList.add("is-hidden");
    $("run-title").textContent = "Running DIA analysis";
    $("run-summary").textContent = next.outputDir;
    $("meter").style.width = "0%";
    showView("run");
    busy(true);
    try {
      await invoke("dia_start", { settings: next });
    } catch (err) {
      finish("Could not start", String(err));
    }
  });
  $("stop").onclick = action(async () => {
    await invoke("cancel_run");
    $("status").textContent = "Stopping…";
  });
  $("clear-log").onclick = () => $("log").replaceChildren();
  $("theme-toggle").onclick = () =>
    document.getElementById("theme-toggle").click();
  $("results-folder").onclick = action(() => openOutputDirectory(resultPath));
  $("view-results").onclick = action(async () => {
    showView("visualizer");
    await openResults(resultPath);
  });
  root.querySelectorAll("[data-dia-view]").forEach((b) => {
    b.onclick = () => showView(b.dataset.diaView);
  });
  if (listen) {
    listen("mk://dia-event", ({ payload: e }) => {
      if (e.type === "stage") {
        $("status").textContent = e.label;
        log(e.label);
      } else if (e.type === "sample") {
        $("status").textContent = `${e.index + 1}/${e.total} ${e.name}`;
      } else if (e.type === "progress") {
        $("meter").style.width = `${e.total ? 100 * e.done / e.total : 0}%`;
      } else if (e.type === "log") log(e.message);
      else if (e.type === "metric") log(`${e.key}: ${e.value}`);
    });
    listen("mk://dia-finished", ({ payload: o }) => {
      resultPath = o.outputDir;
      session = null;
      store("mk-dia-results", resultPath);
      finish(
        "DIA complete",
        `${o.samples} samples · ${o.libraryEntries.toLocaleString()} library entries · ${o.annotations.toLocaleString()} sample matches · ${
          o.elapsedSeconds.toFixed(1)
        } s. Report.csv, Report_fill_all.csv and target_list.csv are ready.`,
      );
      $("meter").style.width = "100%";
      $("results-folder").classList.remove("is-hidden");
      $("view-results").classList.remove("is-hidden");
      openResults(resultPath);
    });
    listen(
      "mk://dia-failed",
      ({ payload }) =>
        finish(
          "DIA failed",
          `${payload} Choose a new output folder before retrying.`,
        ),
    );
    listen("mk://dia-integration-finished", ({ payload: o }) => {
      busy(false);
      $("meter").style.width = "100%";
      const message = `Integrated ${o.samples} samples · ${
        o.targets.toLocaleString()
      } targets · ${o.editedWindows.toLocaleString()} edited windows in ${
        o.elapsedSeconds.toFixed(1)
      } s. Report_integrated.csv and peaks.csv are in the integration folder.`;
      $("status").textContent = "Integration exported";
      $("view-status").textContent = message;
      log(message);
      loadOverview();
    });
    listen("mk://dia-integration-failed", ({ payload }) => {
      busy(false);
      $("status").textContent = "Export failed";
      $("view-status").textContent = String(payload);
      log(String(payload));
    });
    listen("mk://dia-integration-cancelled", () => {
      busy(false);
      $("status").textContent = "Export stopped";
      $("view-status").textContent = "Export stopped.";
      log("Integrated export stopped.");
      loadOverview();
    });
    listen(
      "mk://dia-cancelled",
      () =>
        finish(
          "DIA stopped",
          "Partial results were kept. Choose a new output folder before starting again.",
        ),
    );
  }

  function updatePaging() {
    $("prev").disabled = running || !session || offset === 0;
    $("next").disabled = running || !session || offset + 100 >= session.total;
  }
  // busy() toggles every control; restore the ones that depend on state.
  function refreshControls() {
    $("target-list").querySelectorAll("button").forEach((b) => {
      b.disabled = running || !session?.caches;
    });
    renderExport();
    renderIntegration();
    schedulePaint();
  }
  async function openResults(path) {
    resultPath = path;
    session = null;
    offset = 0;
    selected = null;
    detailData = null;
    overview = null;
    targetWindows = null;
    plan = null;
    pending = null;
    references.clear();
    detailToken++;
    windowsToken++;
    $("search").value = "";
    $("result-path").textContent = path;
    $("charts").replaceChildren();
    charts = [];
    $("browser").classList.add("is-hidden");
    $("target-title").textContent = "";
    $("target-metrics").textContent = "";
    renderExport();
    renderIntegration();
    await loadPage();
    await loadOverview();
    store("mk-dia-results", path);
  }
  function markEdited(button, edited) {
    button.classList.toggle("has-edits", edited > 0);
    button.title = edited
      ? `${button.dataset.name} · ${edited} edited window${
        edited === 1 ? "" : "s"
      }`
      : button.dataset.name;
  }
  async function loadPage() {
    if (!invoke || !resultPath || running) return;
    const token = ++viewerToken;
    $("view-status").textContent = "Reading target index…";
    try {
      const data = await invoke("dia_open", {
        path: resultPath,
        search: $("search").value.trim(),
        offset,
      });
      if (token !== viewerToken) return;
      session = data;
      $("browser").classList.remove("is-hidden");
      $("view-status").textContent = data.caches
        ? `${data.total.toLocaleString()} targets · ${data.samples.length} samples`
        : "Scan caches are absent. Reports remain available; rerun with caches kept to view chromatograms.";
      const oldSample = $("sample").value;
      $("sample").replaceChildren(
        ...data.samples.map((name, i) => new Option(basename(name), String(i))),
      );
      if (oldSample && +oldSample < data.samples.length) {
        $("sample").value = oldSample;
      }
      $("target-list").replaceChildren();
      for (const target of data.targets) {
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = `${target.name} · ${target.mz.toFixed(4)}`;
        button.dataset.target = String(target.id);
        button.dataset.name = target.name;
        button.setAttribute("aria-pressed", String(target.id === selected));
        markEdited(button, target.edited);
        button.disabled = !data.caches;
        button.onclick = () => selectTarget(target.id, button);
        $("target-list").append(button);
      }
      $("page").textContent = data.total
        ? `${offset + 1}–${Math.min(offset + 100, data.total)} / ${data.total}`
        : "0 targets";
      updatePaging();
      renderExport();
    } catch (err) {
      if (token === viewerToken) {
        $("view-status").textContent = String(err);
        $("browser").classList.add("is-hidden");
      }
    }
  }
  async function loadOverview() {
    if (!invoke || !resultPath) return;
    const path = resultPath;
    try {
      const data = await invoke("dia_integration_overview", { path });
      if (path !== resultPath) return;
      overview = data;
      axis = sequenceAxis(data.samples.map((s) => s.acquired));
    } catch (err) {
      overview = null;
      $("view-status").textContent = String(err);
    }
    renderExport();
    renderIntegration();
  }
  function selectTarget(id, button) {
    selected = id;
    plan = null;
    pending = null;
    targetWindows = null;
    $("target-list").querySelectorAll("button").forEach((b) =>
      b.setAttribute("aria-pressed", String(b === button))
    );
    loadTargetWindows();
    requestDetail();
  }
  async function loadTargetWindows() {
    if (selected === null) return;
    const token = ++windowsToken;
    try {
      const data = await invoke("dia_integration_target", {
        path: resultPath,
        target: selected,
      });
      if (token !== windowsToken) return;
      targetWindows = data;
    } catch (err) {
      if (token !== windowsToken) return;
      targetWindows = null;
      $("view-status").textContent = String(err);
    }
    renderIntegration();
  }
  // Coalesce rapid selections: at most one cache extraction is in flight.
  async function requestDetail() {
    detailToken++;
    detailQueued = true;
    if (detailBusy || running) return;
    detailBusy = true;
    try {
      while (detailQueued && !running && selected !== null) {
        detailQueued = false;
        const token = detailToken;
        $("view-status").textContent = "Loading chromatograms…";
        try {
          const data = await invoke("dia_detail", {
            path: resultPath,
            target: selected,
            sample: Number($("sample").value),
            width: $("trace-width").valueAsNumber,
          });
          if (token !== detailToken) continue;
          detailData = data;
          pending = null;
          $("target-title").textContent = data.name;
          $("target-metrics").textContent = `m/z ${fixed(data.mz, 4)} · RT ${
            fixed(data.rt, 3)
          } min · engine area ${fixed(data.area, 1)} · shape ${
            fixed(data.shape, 2)
          } · smoothness ${fixed(data.smoothness, 2)}`;
          $("view-status").textContent = data.present
            ? "Identified in this sample. Intensities are unscaled."
            : "Not identified in this sample; gap-filling window around the cross-sample median RT.";
          buildCharts();
          renderIntegration();
        } catch (err) {
          if (token === detailToken) {
            detailData = null;
            charts = [];
            $("charts").replaceChildren();
            $("view-status").textContent = String(err);
            renderIntegration();
          }
        }
      }
    } finally {
      detailBusy = false;
    }
  }

  /* --- integration window ------------------------------------------------ */

  const viewAction = (fn) => async () => {
    try {
      await fn();
    } catch (err) {
      $("view-status").textContent = String(err);
      log(String(err));
    }
  };
  const SOURCES = {
    engine: "Engine window",
    manual: "Edited",
    sequence: "Interpolated",
  };
  const currentSample = () => Number($("sample").value);
  const currentWindow = () => pending ?? detailData?.window;
  const scanTimes = () => (detailData?.precursor ?? []).map((p) => p[0]);
  const editable = () =>
    !running && !!detailData && !!session?.caches && selected !== null &&
    scanTimes().length >= 2;
  const refsFor = (id) => {
    if (!references.has(id)) references.set(id, new Set());
    return references.get(id);
  };
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
  const area = (v) =>
    Number.isFinite(v)
      ? v.toLocaleString(undefined, {
        minimumFractionDigits: 1,
        maximumFractionDigits: 1,
      })
      : "—";

  function renderExport() {
    const show = !!overview && !!session;
    $("export").classList.toggle("is-hidden", !show);
    if (!show) return;
    const o = overview;
    const edits = o.editedWindows
      ? `${plural(o.editedWindows, "edited window")} in ${
        plural(o.editedTargets, "target")
      }`
      : "Engine windows for every target";
    let state = "not exported yet";
    if (o.exported) {
      state = o.stale
        ? "export out of date"
        : `exported ${
          new Date(o.exported.finishedAt * 1000).toLocaleString(undefined, {
            dateStyle: "medium",
            timeStyle: "short",
          })
        }`;
    }
    $("export-summary").textContent = session.caches
      ? `${edits} · ${state}`
      : "Integration needs the run's scan caches.";
    $("export-summary").classList.toggle("is-stale", !!o.exported && o.stale);
    $("project-baseline").value = o.baseline;
    $("project-baseline").disabled = running || !session.caches;
    $("export-run").disabled = running || !session.caches;
    $("export-open").classList.toggle("is-hidden", !o.exported);
    $("export-open").disabled = running;
  }
  function renderReadout() {
    const w = currentWindow();
    if (!w || !targetWindows) return;
    const s = targetWindows.samples[currentSample()];
    $("int-range").textContent = `${w.start.toFixed(3)} – ${
      w.end.toFixed(3)
    } min`;
    const source = pending ? "pending" : s?.source ?? "engine";
    $("int-source").textContent = pending
      ? "Unsaved"
      : SOURCES[source] ?? source;
    $("int-source").dataset.source = source;
  }
  function renderIntegration() {
    const show = !!(targetWindows && detailData && session?.caches &&
      targetWindows.id === selected);
    $("integration").classList.toggle("is-hidden", !show);
    if (!show) return;
    const samples = targetWindows.samples;
    const s = currentSample();
    const refs = refsFor(targetWindows.id);
    const edited = samples.filter((x) => x.source !== "engine").length;
    renderReadout();
    $("int-baseline").options[0].textContent = `Project default · ${
      baselineLabel(overview?.baseline ?? "none")
    }`;
    $("int-baseline").value = targetWindows.customBaseline
      ? targetWindows.baseline
      : "";
    $("int-baseline").disabled = running;
    $("int-reset").disabled = running ||
      samples[s]?.source === "engine" && !pending;
    const multi = samples.length > 1;
    $("seq-wrap").classList.toggle("is-hidden", !multi);
    $("seq-summary").textContent = `Acquisition sequence · ${
      plural(samples.length, "sample")
    } · ${plural(refs.size, "reference")}`;
    const isRef = refs.has(s);
    $("int-ref").setAttribute("aria-pressed", String(isRef));
    $("int-ref").textContent = isRef ? "Reference ✓" : "Use as reference";
    $("int-ref").disabled = running;
    $("int-preview").disabled = running || !refs.size;
    $("int-apply").disabled = running || !plan?.length;
    $("int-apply").textContent = plan?.length
      ? `Apply to ${plural(plan.length, "sample")}`
      : "Apply";
    if (resetArmed && !edited) disarmReset();
    $("int-reset-all").disabled = running || !edited;
    let hint;
    if (plan) {
      hint = plan.length
        ? `${
          plural(plan.length, "proposed window")
        } shown dashed. Review, then apply.`
        : "The references already match every sample's window.";
    } else if (!refs.size) {
      hint =
        "Mark well-integrated samples as references (Shift-click in the chart), then preview.";
    } else if (refs.size === 1) {
      hint =
        "One reference gives every sample its window. Add references across the run to follow RT drift.";
    } else {
      hint = `Start and end interpolate along ${
        axis.timed ? "acquisition time" : "run order (timestamps unavailable)"
      }; outside the references the nearest one is held.`;
    }
    $("int-hint").textContent = hint;
    drawSequence();
  }

  /* --- chromatograms ------------------------------------------------------ */

  const PLOT = { left: 56, right: 16, top: 15, bottom: 124, height: 150 };
  function xScale(width) {
    const [lo, hi] = detailData.range;
    const span = width - PLOT.left - PLOT.right;
    return {
      x: (v) => PLOT.left + (v - lo) / (hi - lo) * span,
      rt: (px) => lo + (px - PLOT.left) / span * (hi - lo),
    };
  }
  function peakText({ peak, engineArea }) {
    if (!peak.scans) return "no scans in window";
    let text = `area ${area(peak.area)}`;
    if (engineArea != null && Math.abs(engineArea - peak.area) >= 0.05) {
      text += ` (engine ${area(engineArea)})`;
    }
    if (peak.apex != null) text += ` · apex ${peak.apex.toFixed(3)}`;
    if (peak.fwhm != null) text += ` · FWHM ${peak.fwhm.toFixed(3)}`;
    return text;
  }
  // Edge focus survives a chart rebuild only for keyboard users; restoring it
  // after a drag would show a focus ring nobody asked for.
  let keyboardInput = false;
  document.addEventListener("keydown", () => keyboardInput = true, true);
  document.addEventListener("pointerdown", () => keyboardInput = false, true);
  function buildCharts() {
    const focused = keyboardInput &&
      document.activeElement?.closest?.(".dia-edge");
    const focus = focused && [focused.dataset.chart, focused.dataset.edge];
    $("charts").replaceChildren();
    charts = [];
    if (!detailData) return;
    const engine = (value) => detailData.present ? value : null;
    const traces = [
      {
        label: `Precursor · m/z ${fixed(detailData.mz, 4)}`,
        short: "Precursor",
        points: detailData.precursor,
        peak: detailData.precursorPeak,
        engineArea: engine(detailData.area),
      },
      ...detailData.fragments.map((f) => ({
        label: `Fragment ${fixed(f.mz, 4)} · r ${fixed(f.correlation, 2)}`,
        short: `Fragment ${fixed(f.mz, 4)}`,
        points: f.points,
        peak: f.peak,
        engineArea: engine(f.area),
      })),
    ];
    traces.forEach((trace, index) => {
      const figure = document.createElement("figure"),
        caption = document.createElement("figcaption"),
        name = document.createElement("span"),
        stats = document.createElement("span"),
        plot = document.createElement("div"),
        canvas = document.createElement("canvas");
      name.textContent = trace.label;
      stats.className = "dia-peak";
      caption.append(name, stats);
      plot.className = "dia-plot";
      canvas.setAttribute("role", "img");
      canvas.setAttribute(
        "aria-label",
        `${trace.label}, intensity against retention time in minutes`,
      );
      // Only the precursor's edges are in the tab order; all are draggable.
      const handles = ["start", "end"].map((edge) => {
        const handle = document.createElement("button");
        handle.type = "button";
        handle.className = "dia-edge";
        handle.dataset.edge = edge;
        handle.dataset.chart = String(index);
        handle.tabIndex = index === 0 ? 0 : -1;
        handle.setAttribute("role", "slider");
        handle.setAttribute("aria-orientation", "horizontal");
        handle.setAttribute("aria-label", `${trace.short}, integration ${edge}`);
        handle.addEventListener("keydown", (e) => nudge(e, edge));
        return handle;
      });
      plot.append(canvas, ...handles);
      plot.addEventListener("pointerdown", (e) => startDrag(e, index));
      plot.addEventListener("pointermove", moveDrag);
      plot.addEventListener("pointerup", (e) => endDrag(e, true));
      plot.addEventListener("pointercancel", (e) => endDrag(e, false));
      plot.addEventListener("lostpointercapture", (e) => endDrag(e, false));
      figure.append(caption, plot);
      $("charts").append(figure);
      charts.push({ trace, canvas, handles, stats, plot });
    });
    paintCharts();
    if (focus) {
      charts[+focus[0]]?.handles.find((h) => h.dataset.edge === focus[1])
        ?.focus();
    }
  }
  function schedulePaint() {
    cancelAnimationFrame(paintFrame);
    paintFrame = requestAnimationFrame(() => {
      paintCharts();
      renderReadout();
      drawSequence();
    });
  }
  function paintCharts() {
    if (
      !detailData || !charts.length || root.classList.contains("is-hidden") ||
      $("visualizer").classList.contains("is-hidden")
    ) return;
    const style = getComputedStyle(root);
    const color = (name) => style.getPropertyValue(name).trim();
    const proposal = plan?.find((p) => p.sample === currentSample());
    for (const chart of charts) {
      paintChart(chart, currentWindow(), proposal, color);
    }
  }
  function paintChart({ trace, canvas, handles, stats }, window, proposal, color) {
    const width = Math.max(200, canvas.clientWidth),
      height = PLOT.height,
      ratio = Math.min(globalThis.devicePixelRatio || 1, 2);
    if (canvas.width !== Math.round(width * ratio)) {
      canvas.width = Math.round(width * ratio);
      canvas.height = height * ratio;
    }
    const ctx = canvas.getContext("2d");
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const [lo, hi] = detailData.range;
    const { x } = xScale(width);
    const max = trace.points.reduce((m, p) => Math.max(m, p[1]), 1);
    const y = (v) => PLOT.bottom - Math.max(0, v) / max * 100;
    const right = width - PLOT.right;
    ctx.lineWidth = 1;
    ctx.strokeStyle = color("--rule-strong");
    ctx.beginPath();
    ctx.moveTo(PLOT.left, PLOT.top);
    ctx.lineTo(PLOT.left, PLOT.bottom);
    ctx.lineTo(right, PLOT.bottom);
    ctx.stroke();
    ctx.font = "10px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.fillStyle = color("--ink-2");
    ctx.fillText(max.toPrecision(3), 0, 25);
    ctx.fillText("0", 35, PLOT.bottom);
    ctx.fillText(lo.toFixed(2), PLOT.left, 142);
    ctx.textAlign = "right";
    ctx.fillText(`${hi.toFixed(2)} min`, right, 142);
    ctx.textAlign = "left";
    ctx.save();
    ctx.beginPath();
    ctx.rect(PLOT.left, 0, right - PLOT.left, PLOT.bottom + 1);
    ctx.clip();
    const vline = (v, dash, stroke, lineWidth = 1) => {
      ctx.setLineDash(dash);
      ctx.lineWidth = lineWidth;
      ctx.strokeStyle = stroke;
      ctx.beginPath();
      ctx.moveTo(Math.round(x(v)) + .5, PLOT.top);
      ctx.lineTo(Math.round(x(v)) + .5, PLOT.bottom);
      ctx.stroke();
      ctx.setLineDash([]);
    };
    // Integrated region: the window's scans above the baseline.
    const inside = trace.points.filter(([t]) =>
      t >= window.start && t < window.end
    );
    const baseline = pending
      ? previewBaseline(trace.points, window, detailData.baseline)
      : { b0: trace.peak.baseline[0], b1: trace.peak.baseline[1] };
    if (inside.length >= 2 && baseline) {
      const t0 = inside[0][0], t1 = inside[inside.length - 1][0];
      const base = (t) =>
        baseline.b0 + (t - t0) / (t1 - t0) * (baseline.b1 - baseline.b0);
      ctx.beginPath();
      inside.forEach(([t, v], i) => {
        const py = y(Math.max(v, base(t)));
        if (i) ctx.lineTo(x(t), py);
        else ctx.moveTo(x(t), py);
      });
      ctx.lineTo(x(t1), y(baseline.b1));
      ctx.lineTo(x(t0), y(baseline.b0));
      ctx.closePath();
      ctx.globalAlpha = .16;
      ctx.fillStyle = color("--accent");
      ctx.fill();
      ctx.globalAlpha = 1;
      if (detailData.baseline !== "none") {
        ctx.setLineDash([4, 3]);
        ctx.strokeStyle = color("--ink-2");
        ctx.beginPath();
        ctx.moveTo(x(t0), y(baseline.b0));
        ctx.lineTo(x(t1), y(baseline.b1));
        ctx.stroke();
        ctx.setLineDash([]);
      }
    }
    if (!sameWindow(window, detailData.engineWindow)) {
      vline(detailData.engineWindow.start, [1, 3], color("--ink-3"));
      vline(detailData.engineWindow.end, [1, 3], color("--ink-3"));
    }
    if (proposal) {
      vline(proposal.start, [5, 3], color("--warn"), 1.5);
      vline(proposal.end, [5, 3], color("--warn"), 1.5);
    }
    vline(detailData.rt, [3, 3], color("--rule-strong"));
    const stroke = (points, style, lineWidth) => {
      if (!points.length) return;
      ctx.strokeStyle = style;
      ctx.lineWidth = lineWidth;
      ctx.lineJoin = "round";
      ctx.beginPath();
      points.forEach(([t, v], i) => {
        if (i) ctx.lineTo(x(t), y(v));
        else ctx.moveTo(x(t), y(v));
      });
      ctx.stroke();
    };
    stroke(trace.points, color("--ink-3"), 1.25);
    stroke(inside, color("--accent"), 1.75);
    ctx.restore();
    if (!trace.points.length) {
      ctx.fillStyle = color("--ink-2");
      ctx.fillText("No scans in this RT interval", 64, 66);
    }
    const enabled = editable();
    for (const handle of handles) {
      const value = window[handle.dataset.edge];
      const px = x(value);
      handle.style.display = px < PLOT.left - 1 || px > right + 1 ? "none" : "";
      handle.style.left = `${px}px`;
      handle.disabled = !enabled;
      handle.setAttribute("aria-valuemin", lo.toFixed(3));
      handle.setAttribute("aria-valuemax", hi.toFixed(3));
      handle.setAttribute("aria-valuenow", value.toFixed(3));
      handle.setAttribute("aria-valuetext", `${value.toFixed(3)} minutes`);
    }
    stats.textContent = pending ? "…" : peakText(trace);
  }
  function startDrag(e, index) {
    if (e.button !== 0 || !editable()) return;
    const chart = charts[index];
    const box = chart.canvas.getBoundingClientRect();
    const px = e.clientX - box.left;
    const handle = e.target.closest(".dia-edge");
    if (
      !handle &&
      (px < PLOT.left || px > box.width - PLOT.right ||
        e.clientY - box.top > PLOT.bottom + 4)
    ) return;
    clearTimeout(nudgeTimer);
    drag = {
      edge: handle?.dataset.edge ?? null,
      anchor: xScale(box.width).rt(px),
      origin: currentWindow(),
      chart,
      pointer: e.pointerId,
      moved: false,
    };
    chart.plot.setPointerCapture(e.pointerId);
    chart.plot.classList.add("is-dragging");
    e.preventDefault();
  }
  function moveDrag(e) {
    if (!drag || e.pointerId !== drag.pointer) return;
    const box = drag.chart.canvas.getBoundingClientRect();
    const { x, rt } = xScale(box.width);
    const px = Math.max(
      PLOT.left,
      Math.min(box.width - PLOT.right, e.clientX - box.left),
    );
    const at = rt(px);
    if (!drag.moved && Math.abs(px - x(drag.anchor)) < 3) return;
    drag.moved = true;
    pending = drag.edge
      ? moveEdge(scanTimes(), drag.origin, drag.edge, at)
      : drawnWindow(scanTimes(), drag.anchor, at);
    schedulePaint();
  }
  function endDrag(e, commit) {
    if (!drag || e.pointerId !== drag.pointer) return;
    const { moved, origin, chart } = drag;
    drag = null;
    chart.plot.classList.remove("is-dragging");
    if (chart.plot.hasPointerCapture(e.pointerId)) {
      chart.plot.releasePointerCapture(e.pointerId);
    }
    if (commit && moved && pending && !sameWindow(pending, origin)) {
      commitWindow(pending);
    } else {
      pending = null;
      schedulePaint();
    }
  }
  function nudge(e, edge) {
    if (e.key === "Escape" && pending && !drag) {
      clearTimeout(nudgeTimer);
      pending = null;
      schedulePaint();
      return;
    }
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    if (!editable()) return;
    const steps = (e.key === "ArrowLeft" ? -1 : 1) * (e.shiftKey ? 5 : 1);
    pending = nudgeEdge(scanTimes(), currentWindow(), edge, steps);
    schedulePaint();
    clearTimeout(nudgeTimer);
    nudgeTimer = setTimeout(() => {
      if (pending && !sameWindow(pending, detailData?.window)) {
        commitWindow(pending);
      }
    }, 450);
  }
  async function saveWindows(changes) {
    const target = selected;
    const data = await invoke("dia_integration_set_windows", {
      path: resultPath,
      target,
      changes,
    });
    if (target !== selected) return;
    targetWindows = data;
    plan = null;
    const button = $("target-list").querySelector(`[data-target="${target}"]`);
    if (button) {
      markEdited(
        button,
        data.samples.filter((s) => s.source !== "engine").length,
      );
    }
    renderIntegration();
    loadOverview();
    await requestDetail();
  }
  async function commitWindow(window) {
    if (!editable()) return;
    pending = window;
    schedulePaint();
    try {
      await saveWindows([{ sample: currentSample(), window, source: "manual" }]);
    } catch (err) {
      pending = null;
      schedulePaint();
      $("view-status").textContent = String(err);
    }
  }
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && drag) {
      const { chart, pointer } = drag;
      drag = null;
      pending = null;
      chart.plot.classList.remove("is-dragging");
      if (chart.plot.hasPointerCapture(pointer)) {
        chart.plot.releasePointerCapture(pointer);
      }
      schedulePaint();
    }
  });

  /* --- acquisition sequence ---------------------------------------------- */

  const SVG = "http://www.w3.org/2000/svg";
  function svg(name, attrs, parent) {
    const node = document.createElementNS(SVG, name);
    for (const [key, value] of Object.entries(attrs)) {
      node.setAttribute(key, String(value));
    }
    parent?.append(node);
    return node;
  }
  function niceTicks(lo, hi, count) {
    const span = hi - lo;
    if (!(span > 0)) return [lo];
    const raw = span / count, mag = 10 ** Math.floor(Math.log10(raw));
    const norm = raw / mag;
    const step = (norm < 1.5 ? 1 : norm < 3 ? 2 : norm < 7 ? 5 : 10) * mag;
    const ticks = [];
    for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) {
      ticks.push(Math.round(v / step) * step);
    }
    return ticks;
  }
  function drawSequence() {
    const host = $("seq");
    if (
      !targetWindows || targetWindows.samples.length < 2 ||
      !$("seq-wrap").open || $("integration").classList.contains("is-hidden")
    ) {
      host.replaceChildren();
      return;
    }
    const samples = targetWindows.samples;
    const positions = axis.positions.length === samples.length
      ? axis.positions
      : samples.map((_, i) => i);
    const timed = axis.timed && axis.positions.length === samples.length;
    const current = currentSample();
    const refs = refsFor(targetWindows.id);
    const proposed = new Map((plan ?? []).map((p) => [p.sample, p]));
    const win = (i) =>
      i === current && pending ? pending : samples[i].window;
    const width = Math.max(280, host.clientWidth), height = 138;
    const L = PLOT.left, R = width - PLOT.right, T = 8, B = 106;
    let p0 = Math.min(...positions), p1 = Math.max(...positions);
    const ppad = (p1 - p0) * .03 || 1;
    p0 -= ppad;
    p1 += ppad;
    const values = samples.flatMap((_, i) => [win(i).start, win(i).end])
      .concat([...proposed.values()].flatMap((p) => [p.start, p.end]));
    let v0 = Math.min(...values), v1 = Math.max(...values);
    const vpad = Math.max((v1 - v0) * .15, .01);
    v0 -= vpad;
    v1 += vpad;
    const x = (p) => L + (p - p0) / (p1 - p0) * (R - L);
    const y = (v) => B - (v - v0) / (v1 - v0) * (B - T);
    const chart = svg("svg", {
      width,
      height,
      viewBox: `0 0 ${width} ${height}`,
      role: "group",
      "aria-label": `Integration windows for ${samples.length} samples by ${
        timed ? "acquisition time" : "run order"
      }. Click a sample to view it; Shift-click to toggle it as a reference.`,
    });
    for (const v of niceTicks(v0, v1, 3)) {
      svg("line", { class: "seq-grid", x1: L, x2: R, y1: y(v), y2: y(v) }, chart);
      svg("text", { class: "seq-tick", x: L - 8, y: y(v) + 3, "text-anchor": "end" }, chart)
        .textContent = v.toFixed(2);
    }
    if (timed) {
      const first = Math.min(...positions);
      const span = Math.max(...positions) - first;
      const [unit, suffix] = span >= 7.2e6
        ? [3.6e6, "h"]
        : span >= 1.2e5
        ? [6e4, "min"]
        : [1e3, "s"];
      for (const v of niceTicks((p0 - first) / unit, (p1 - first) / unit, 4)) {
        if (v < 0) continue;
        svg("text", { class: "seq-tick", x: x(first + v * unit), y: B + 15, "text-anchor": "middle" }, chart)
          .textContent = `${+v.toFixed(2)} ${suffix}`;
      }
    } else {
      for (const v of niceTicks(0, samples.length - 1, 5)) {
        if (!Number.isInteger(v)) continue;
        svg("text", { class: "seq-tick", x: x(v), y: B + 15, "text-anchor": "middle" }, chart)
          .textContent = String(v + 1);
      }
    }
    svg("text", { class: "seq-caption", x: R, y: height - 2, "text-anchor": "end" }, chart)
      .textContent = timed ? "elapsed acquisition time" : "run order";
    svg("text", { class: "seq-caption", x: 0, y: T + 2 }, chart).textContent = "RT";
    const order = positions.map((_, i) => i)
      .sort((a, b) => positions[a] - positions[b] || a - b);
    for (const edge of ["start", "end"]) {
      svg("polyline", {
        class: "seq-line",
        points: order.map((i) => `${x(positions[i])},${y(win(i)[edge])}`).join(" "),
      }, chart);
      if (proposed.size) {
        svg("polyline", {
          class: "seq-proposal",
          points: order.map((i) => {
            const w = proposed.get(i) ?? win(i);
            return `${x(positions[i])},${y(w[edge])}`;
          }).join(" "),
        }, chart);
      }
    }
    order.forEach((i, k) => {
      const s = samples[i], w = win(i), px = x(positions[i]);
      const classes = ["seq-sample"];
      if (i === current) classes.push("is-current");
      if (refs.has(i)) classes.push("is-ref");
      if (s.source !== "engine" || (i === current && pending)) {
        classes.push("is-edited");
      }
      if (!s.present) classes.push("is-filled");
      const g = svg("g", { class: classes.join(" ") }, chart);
      const prev = k ? x(positions[order[k - 1]]) : L;
      const next = k < order.length - 1 ? x(positions[order[k + 1]]) : R;
      const left = k ? (prev + px) / 2 : L, rightEdge = k < order.length - 1 ? (px + next) / 2 : R;
      svg("rect", { class: "seq-hit", x: left, y: T, width: Math.max(1, rightEdge - left), height: B - T }, g);
      const band = Math.max(4, Math.min(16, rightEdge - left));
      svg("rect", { class: "seq-band", x: px - band / 2, y: T, width: band, height: B - T, rx: 2 }, g);
      svg("line", { class: "seq-bar", x1: px, x2: px, y1: y(w.start), y2: y(w.end) }, g);
      for (const v of [w.start, w.end]) {
        svg("circle", { class: "seq-dot", cx: px, cy: y(v), r: refs.has(i) ? 3.5 : 2.5 }, g);
      }
      const p = proposed.get(i);
      if (p) {
        for (const v of [p.start, p.end]) {
          svg("line", { class: "seq-proposal-tick", x1: px - 4, x2: px + 4, y1: y(v), y2: y(v) }, g);
        }
      }
      const acquired = overview?.samples[i]?.acquired;
      svg("title", {}, g).textContent = [
        basename(session?.samples[i] ?? `Sample ${i + 1}`),
        `${w.start.toFixed(3)}–${w.end.toFixed(3)} min · ${
          SOURCES[s.source] ?? s.source
        }${s.present ? "" : " · gap-filled"}`,
        refs.has(i) ? "Reference" : "",
        acquired ? `Acquired ${acquired}` : "",
      ].filter(Boolean).join("\n");
      g.addEventListener("click", (e) => {
        if (running) return;
        if (e.shiftKey || e.metaKey || e.ctrlKey) toggleReference(i);
        else chooseSample(i);
      });
    });
    host.replaceChildren(chart);
  }
  function chooseSample(i) {
    if (i === currentSample()) return;
    $("sample").value = String(i);
    onSampleChange();
  }
  function onSampleChange() {
    clearTimeout(nudgeTimer);
    pending = null;
    renderIntegration();
    requestDetail();
  }
  function toggleReference(i) {
    if (!targetWindows) return;
    const refs = refsFor(targetWindows.id);
    if (refs.has(i)) refs.delete(i);
    else refs.add(i);
    plan = null;
    renderIntegration();
    schedulePaint();
  }
  function disarmReset() {
    clearTimeout(resetArmed);
    resetArmed = 0;
    $("int-reset-all").textContent = "Reset all samples";
    $("int-reset-all").classList.remove("is-armed");
  }

  $("int-ref").onclick = () => toggleReference(currentSample());
  $("int-preview").onclick = viewAction(() => {
    const windows = targetWindows.samples.map((s) => s.window);
    plan = sequencePlan(axis.positions.length === windows.length ? axis.positions : windows.map((_, i) => i), windows, [...refsFor(targetWindows.id)])
      .filter((p) => !sameWindow(p, windows[p.sample]));
    renderIntegration();
    schedulePaint();
  });
  $("int-apply").onclick = viewAction(async () => {
    if (!plan?.length) return;
    const count = plan.length, id = targetWindows.id;
    await saveWindows(plan.map((p) => ({
      sample: p.sample,
      window: { start: p.start, end: p.end },
      source: "sequence",
    })));
    refsFor(id).clear();
    renderIntegration();
    $("int-hint").textContent = `Applied interpolated windows to ${
      plural(count, "sample")
    }.`;
  });
  $("int-reset").onclick = viewAction(async () => {
    clearTimeout(nudgeTimer);
    pending = null;
    await saveWindows([{ sample: currentSample(), window: null, source: "manual" }]);
  });
  $("int-reset-all").onclick = viewAction(async () => {
    if (!resetArmed) {
      $("int-reset-all").textContent = "Confirm reset";
      $("int-reset-all").classList.add("is-armed");
      resetArmed = setTimeout(disarmReset, 4000);
      return;
    }
    disarmReset();
    await saveWindows(
      targetWindows.samples
        .map((s, sample) => ({ s, sample }))
        .filter(({ s }) => s.source !== "engine")
        .map(({ sample }) => ({ sample, window: null, source: "manual" })),
    );
  });
  $("int-baseline").onchange = viewAction(async () => {
    await invoke("dia_integration_set_baseline", {
      path: resultPath,
      target: selected,
      baseline: $("int-baseline").value || null,
    });
    await loadTargetWindows();
    loadOverview();
    await requestDetail();
  });
  $("project-baseline").onchange = viewAction(async () => {
    await invoke("dia_integration_set_baseline", {
      path: resultPath,
      target: null,
      baseline: $("project-baseline").value,
    });
    await loadOverview();
    if (selected !== null) {
      await loadTargetWindows();
      await requestDetail();
    }
  });
  $("export-run").onclick = viewAction(async () => {
    if (!resultPath) return;
    clearTimeout(nudgeTimer);
    pending = null;
    $("meter").style.width = "0%";
    $("status").textContent = "Integrating…";
    $("view-status").textContent =
      "Integrating every target in every sample with the saved windows…";
    log(`Exporting integrated areas for ${resultPath}`);
    busy(true);
    try {
      await invoke("dia_integration_export", { path: resultPath });
    } catch (err) {
      busy(false);
      throw err;
    }
  });
  $("export-open").onclick = viewAction(() =>
    openOutputDirectory(
      String(overview?.exported?.report ?? "").replace(/[\\/][^\\/]*$/, ""),
    )
  );
  $("seq-wrap").addEventListener("toggle", () => {
    store("mk-dia-sequence", $("seq-wrap").open ? "open" : "closed");
    drawSequence();
  });

  function redraw() {
    paintCharts();
    drawSequence();
  }
  $("open-results").onclick = action(async () => {
    const path = asPath(
      await dialogOpen({ directory: true, title: "Open DIA result folder" }),
    );
    if (path) await openResults(path);
  });
  $("prev").onclick = action(async () => {
    offset = Math.max(0, offset - 100);
    await loadPage();
  });
  $("next").onclick = action(async () => {
    offset += 100;
    await loadPage();
  });
  $("search").oninput = () => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      offset = 0;
      loadPage();
    }, 200);
  };
  $("sample").onchange = onSampleChange;
  $("trace-width").onchange = requestDetail;
  let redrawFrame = 0;
  // Canvas width is driven by the trace column; height changes don't redraw.
  let lastWidth = 0;
  const columnResize = new ResizeObserver(([entry]) => {
    if (entry.contentRect.width !== lastWidth) {
      lastWidth = entry.contentRect.width;
      cancelAnimationFrame(redrawFrame);
      redrawFrame = requestAnimationFrame(redraw);
    }
  });
  columnResize.observe(root.querySelector(".dia-traces"));
  new MutationObserver(redraw).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
  });

  async function activate(mode, focus = false) {
    document.getElementById("dda-shell").classList.toggle(
      "is-hidden",
      mode !== "dda",
    );
    root.classList.toggle("is-hidden", mode !== "dia");
    if (focus) document.querySelector(`#${mode}-shell .mode-switch`).focus();
    store("mk-acquisition", mode);
    window.dispatchEvent(new Event("resize"));
    redraw();
    if (mode === "dia" && !restored && invoke) {
      restored = true;
      try {
        const path = localStorage.getItem("mk-dia-dataset");
        if (path && await invoke("directory_exists", { path })) {
          await scan(path);
        }
      } catch (err) {
        log(String(err));
      }
    }
  }
  document.querySelectorAll("[data-acquisition]").forEach((button) => {
    button.onclick = () => activate(button.dataset.acquisition, true);
  });
  try {
    if (localStorage.getItem("mk-acquisition") === "dia") activate("dia");
  } catch { /* optional */ }
}
