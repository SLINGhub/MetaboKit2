// Run with Node + Playwright installed. Optional: METABOKIT_DIA_RESULTS points
// to an actual completed run; `cargo build -p metabokit-dia --release --examples`.
const { chromium } = require("playwright");
const { createServer } = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const assert = require("node:assert/strict");
const project = path.resolve(__dirname, "..");
const out = path.join(project, "target", "ui-smoke");
fs.mkdirSync(out, { recursive: true });
const result = process.env.METABOKIT_DIA_RESULTS;
const settings = result ? JSON.parse(fs.readFileSync(path.join(result, "dia-settings.json"))) : {
  mzmlFiles: ["/dataset/sample.mzML"], outputDir: "/dataset/results_dia", libraries: ["user /dataset/library.txt"], libraryLabels: {}, libsDir: null,
  positive: true, ms1Tol: .008, ms2Tol: .05, ms2Score: .5, minPeaks: 1, rtShift: .1, peakWidth: [.05, .8], windows: [[200, 480], [479, 496]],
  peakShape: .1, snScore: .4, integrationRt: 2, integrationMz: .01, imputeWidth: 0, threads: 0, maxFilesInFlight: 1, keepCache: true,
};
const fixture = result ? JSON.parse(execFileSync(path.join(project, "target/release/examples/inspect"), [result], { encoding: "utf8" })) : {
  session: { samples: ["sample.mzML", "second.mzML"], targets: [{ id: 0, name: "Example", mz: 400, edited: 0 }], total: 1, caches: true },
  detail: { name: "Example", mz: 400, rt: 1, present: true, area: 30, shape: .9, smoothness: .8, precursor: [[.8, 1], [.9, 4], [1, 10], [1.1, 4], [1.2, 1]], fragments: [{ mz: 200, correlation: .9, area: 20, points: [[.8, 1], [.9, 3], [1, 8], [1.1, 3], [1.2, 1]], peak: { area: 20, height: 8, apex: 1, fwhm: .2, baseline: [0, 0], scans: 2 } }],
    range: [.7, 1.3], window: { start: .95, end: 1.15 }, engineWindow: { start: .95, end: 1.15 }, source: "engine", baseline: "none", precursorPeak: { area: 30, height: 10, apex: 1, fwhm: .2, baseline: [0, 0], scans: 2 } },
  overview: { samples: [{ name: "sample.mzML", acquired: "2026-09-30T12:00:00Z" }, { name: "second.mzML", acquired: "2026-09-30T13:00:00Z" }], baseline: "none", editedWindows: 0, editedTargets: 0, exported: null, stale: false },
  windows: { id: 0, name: "Example", baseline: "none", customBaseline: false, samples: [{ present: true, rt: 1, engine: { start: .95, end: 1.15 }, window: { start: .95, end: 1.15 }, source: "engine" }, { present: false, rt: 1, engine: { start: .95, end: 1.15 }, window: { start: .95, end: 1.15 }, source: "engine" }] },
};
const server = createServer((req, res) => {
  const file = path.join(project, "ui", req.url === "/" ? "index.html" : req.url);
  if (!file.startsWith(path.join(project, "ui") + path.sep)) { res.writeHead(403).end(); return; }
  fs.readFile(file, (error, data) => { if (error) res.writeHead(404).end(); else { res.setHeader("Content-Type", file.endsWith(".js") ? "text/javascript" : file.endsWith(".css") ? "text/css" : "text/html"); res.end(data); } });
});
(async () => {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  let browser;
  try {
    browser = await chromium.launch({ headless: true, channel: process.env.METABOKIT_BROWSER_CHANNEL || "chrome" });
    const page = await browser.newPage({ viewport: { width: 1320, height: 880 }, deviceScaleFactor: 1 });
    const errors = []; page.on("pageerror", (e) => errors.push(String(e)));
    await page.addInitScript(({ settings, fixture }) => {
      const handlers = new Map();
      window.__testEmit = (name, payload) => handlers.get(name)?.({ payload });
      window.__testCalls = [];
      window.__TAURI__ = {
        event: { listen: async (name, cb) => { handlers.set(name, cb); return () => handlers.delete(name); } },
        core: { invoke: async (command, args) => {
          window.__testCalls.push({ command, args });
          if (command === "default_params") return { mzmlFiles: [], libraries: [], outputDir: "", peakWidth: [.05, .8] };
          if (command === "validate_params" || command === "builtin_libraries") return [];
          if (command === "system_info") return { version: "0.3.0", os: "macOS", availableThreads: 8 };
          if (command === "directory_exists") return true;
          if (command === "is_running") return false;
          if (command === "plugin:dialog|open") return args.options.title?.includes("result") ? "/fixture/results" : "/fixture/dataset";
          if (command === "dia_scan") return { settings, totalBytes: 463000000, notes: ["Imported original DIA configuration."] };
          if (command === "dia_validate" || command === "dia_start" || command === "cancel_run") return null;
          if (command === "dia_open") return fixture.session;
          if (command === "dia_detail") return fixture.detail;
          if (command === "dia_integration_overview") return fixture.overview;
          if (command === "dia_integration_target") return fixture.windows;
          if (command === "dia_integration_set_windows") {
            const [change] = args.changes;
            const samples = fixture.windows.samples.map((s, i) => i === change.sample ? { ...s, window: change.window ?? s.engine, source: change.window ? change.source : "engine" } : s);
            return { ...fixture.windows, samples };
          }
          throw new Error(`Unexpected native command: ${command}`);
        } },
      };
    }, { settings, fixture });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForFunction(() => document.querySelector("[data-app-version]").textContent.includes("0.3.0"));
    assert.equal(await page.locator(".acquisition-tabs").count(), 0);
    await page.locator("#dda-shell .mode-switch").click();
    await page.locator("#dia-choose").click();
    await page.waitForFunction(() => !document.querySelector("#dia-start").disabled);
    assert.equal(await page.locator("#dia-files li").count(), settings.mzmlFiles.length);
    await page.locator("#dia-ms1Tol").evaluate((e) => { e.value = ".012"; e.dispatchEvent(new Event("input", { bubbles: true })); });
    await page.locator("#dia-shell .mode-switch").click();
    await page.locator("#dda-shell .mode-switch").click();
    assert.equal(Number(await page.locator("#dia-ms1Tol").inputValue()), 0.012);
    await page.screenshot({ path: path.join(out, "dia-dataset.png") });
    await page.reload();
    await page.waitForFunction(() => !document.querySelector("#dia-shell").classList.contains("is-hidden") && !document.querySelector("#dia-start").disabled);
    assert.equal(await page.evaluate(() => localStorage.getItem("mk-acquisition")), "dia");
    await page.locator("[data-dia-view=visualizer]").click();
    await page.locator("#dia-open-results").click();
    await page.locator("#dia-target-list button").first().click();
    await page.waitForFunction(() => document.querySelectorAll("#dia-charts canvas").length >= 2);
    await page.screenshot({ path: path.join(out, "dia-chromatograms.png") });
    assert.equal(await page.locator("#dia-charts canvas").count(), fixture.detail.fragments.length + 1);
    // Integration: window readout, two draggable edges per trace, and a
    // keyboard nudge that saves a manual window.
    await page.waitForFunction(() => !document.querySelector("#dia-integration").classList.contains("is-hidden"));
    assert.equal(await page.locator("#dia-charts .dia-edge").count(), 2 * (fixture.detail.fragments.length + 1));
    assert.equal(await page.locator(".seq-sample").count(), fixture.session.samples.length);
    await page.locator("#dia-charts .dia-edge[data-edge=end]").first().focus();
    await page.keyboard.press("ArrowRight");
    await page.waitForFunction(() => window.__testCalls.some((c) => c.command === "dia_integration_set_windows"));
    const saved = await page.evaluate(() => window.__testCalls.find((c) => c.command === "dia_integration_set_windows").args.changes[0]);
    assert.equal(saved.source, "manual");
    assert.ok(saved.window.end > fixture.detail.window.end);
    await page.screenshot({ path: path.join(out, "dia-integration.png") });
    await page.setViewportSize({ width: 1000, height: 660 });
    await page.screenshot({ path: path.join(out, "dia-small-window.png") });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false);
    await page.setViewportSize({ width: 1320, height: 880 });
    await page.locator("#dia-start").click();
    assert.equal(await page.locator("#dia-shell .mode-switch").isDisabled(), true);
    await page.evaluate(() => window.__testEmit("mk://dia-event", { type: "stage", label: "Processing samples" }));
    await page.locator("#dia-stop").click();
    await page.evaluate(() => window.__testEmit("mk://dia-cancelled", null));
    await page.waitForFunction(() => document.querySelector("#dia-run-title").textContent === "DIA stopped");
    await page.locator("#dia-start").click();
    await page.evaluate(() => window.__testEmit("mk://dia-finished", {outputDir:"/fixture/new-results", samples:3, libraryEntries:3634, annotations:9677, elapsedSeconds:2.5}));
    await page.waitForFunction(() => document.querySelector("#dia-result-path").textContent === "/fixture/new-results" && document.querySelectorAll("#dia-target-list button").length > 0);
    await page.locator("[data-dia-view=visualizer]").click();
    await page.locator("#dia-target-list button").first().click();
    await page.waitForFunction(() => document.querySelectorAll("#dia-charts canvas").length >= 2);
    assert.equal(await page.evaluate(() => window.__testCalls.filter(c => c.command === "dia_detail").at(-1).args.path), "/fixture/new-results");
    await page.locator("#dia-shell .mode-switch").click();
    await page.reload();
    await page.waitForFunction(() => !document.querySelector("#dda-shell").classList.contains("is-hidden"));
    assert.equal(await page.evaluate(() => localStorage.getItem("mk-acquisition")), "dda");
    assert.deepEqual(errors, []);
    console.log("PASS: label toggle, remembered mode, independent settings, dataset controls, paged traces, integration edges and keyboard edits, small-window layout, run/cancel events; no JavaScript errors.");
  } finally { if (browser) await browser.close(); server.close(); }
})().catch((e) => { console.error(e); process.exitCode = 1; });
