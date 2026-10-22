/* Integration-window geometry and acquisition-sequence interpolation for the
 * DIA visualizer. Pure functions only; areas are always computed natively.
 * Sequence interpolation follows MRMhub's Visualizer (sequence.js). */

export const BASELINES = [
  ["none", "None · engine area"],
  ["valley-to-valley", "Valley to valley"],
  ["vertical-drop", "Vertical drop"],
  ["percentile", "5th percentile"],
];
export const baselineLabel = (value) =>
  (BASELINES.find(([v]) => v === value) ?? BASELINES[0])[1];

// Acquisition metadata is the mzML run startTimeStamp kept at the head of
// each MS1 cache, not a file date. Unzoned instrument clocks are read as UTC
// so ordering never depends on this computer's time zone.
export function acquisitionTime(value) {
  const text = String(value ?? "").trim();
  if (
    !/^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})?$/
      .test(text)
  ) return null;
  const normalized = text.replace(" ", "T");
  const day = new Date(`${normalized.slice(0, 10)}T00:00:00Z`);
  if (
    !Number.isFinite(day.getTime()) ||
    day.toISOString().slice(0, 10) !== normalized.slice(0, 10)
  ) return null;
  const ms = Date.parse(
    /(?:Z|[+-]\d{2}:\d{2})$/.test(normalized) ? normalized : `${normalized}Z`,
  );
  return Number.isFinite(ms) ? ms : null;
}

// Elapsed acquisition time when every sample has a distinct timestamp;
// otherwise run order, so samples never overlap or disappear.
export function sequenceAxis(acquired) {
  const times = acquired.map(acquisitionTime);
  const timed = times.length > 1 && times.every((t) => t != null) &&
    new Set(times).size === times.length;
  return { timed, positions: timed ? times : times.map((_, i) => i) };
}

// Piecewise-linear start/end boundaries through the reference windows, held
// at the nearest reference outside their range (never extrapolated). Returns
// a window for every non-reference sample. This proposes bounds; it does not
// detect peaks.
export function sequencePlan(positions, windows, references) {
  const anchors = [...new Set(references)]
    .filter((i) => windows[i] && Number.isFinite(positions[i]))
    .map((i) => ({ i, position: positions[i], ...windows[i] }))
    .sort((a, b) => a.position - b.position || a.i - b.i);
  if (!anchors.length) return [];
  for (const a of anchors) {
    if (!(a.start >= 0 && a.end > a.start)) {
      throw new Error("A reference has an invalid integration window.");
    }
  }
  const isAnchor = new Set(anchors.map((a) => a.i));
  const order = positions.map((_, i) => i)
    .sort((a, b) => positions[a] - positions[b] || a - b);
  const plan = [];
  let right = 0;
  for (const i of order) {
    if (isAnchor.has(i)) continue;
    const p = positions[i];
    while (right < anchors.length - 1 && anchors[right].position < p) right++;
    const r = anchors[right];
    const l = anchors[Math.max(0, right - (r.position > p ? 1 : 0))];
    const f = r.position === l.position
      ? 0
      : Math.max(0, Math.min(1, (p - l.position) / (r.position - l.position)));
    plan.push({
      sample: i,
      start: l.start + f * (r.start - l.start),
      end: l.end + f * (r.end - l.end),
    });
  }
  return plan.sort((a, b) => a.sample - b.sample);
}

// Scans are integrated when start <= rt < end, as in the engine.
export function includedScans(times, window) {
  let first = 0;
  while (first < times.length && times[first] < window.start) first++;
  let last = first - 1;
  while (last + 1 < times.length && times[last + 1] < window.end) last++;
  return last >= first ? [first, last] : null;
}

export function nearestScan(times, rt) {
  let lo = 0, hi = times.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid] < rt) lo = mid + 1;
    else hi = mid;
  }
  return lo > 0 && rt - times[lo - 1] <= times[lo] - rt ? lo - 1 : lo;
}

// Edited edges sit midway between scans, so a window includes exactly the
// scans its edges were placed on, independent of float rounding.
export function edgeAt(times, index, edge) {
  const n = times.length;
  const gap = (a, b) => (b - a) / 2;
  if (edge === "start") {
    return index > 0
      ? times[index - 1] + gap(times[index - 1], times[index])
      : times[0] - gap(times[0], times[1]);
  }
  return index < n - 1
    ? times[index] + gap(times[index], times[index + 1])
    : times[n - 1] + gap(times[n - 2], times[n - 1]);
}

const scanRange = (times, window) =>
  includedScans(times, window) ??
    [
      nearestScan(times, window.start),
      Math.max(
        nearestScan(times, window.start) + 1,
        nearestScan(times, window.end),
      ),
    ];

// Moves one edge to the scan nearest `rt`, keeping at least two scans.
export function moveEdge(times, window, edge, rt) {
  if (times.length < 2) return window;
  const [first, last] = scanRange(times, window);
  const i = nearestScan(times, rt);
  return edge === "start"
    ? { ...window, start: edgeAt(times, Math.min(i, last - 1), "start") }
    : { ...window, end: edgeAt(times, Math.max(i, first + 1), "end") };
}

// Moves one edge by whole scans, keeping at least two scans.
export function nudgeEdge(times, window, edge, steps) {
  if (times.length < 2) return window;
  const [first, last] = scanRange(times, window);
  if (edge === "start") {
    const i = Math.max(0, Math.min(last - 1, first + steps));
    return { ...window, start: edgeAt(times, i, "start") };
  }
  const i = Math.min(times.length - 1, Math.max(first + 1, last + steps));
  return { ...window, end: edgeAt(times, i, "end") };
}

// A new window drawn between two RTs, spanning at least two scans.
export function drawnWindow(times, a, b) {
  if (times.length < 2) return null;
  let lo = nearestScan(times, Math.min(a, b));
  let hi = nearestScan(times, Math.max(a, b));
  if (hi <= lo) {
    if (lo < times.length - 1) hi = lo + 1;
    else lo = hi - 1;
  }
  return { start: edgeAt(times, lo, "start"), end: edgeAt(times, hi, "end") };
}

// Display-only baseline while a window is being dragged; the native result
// replaces it once the edit is saved.
export function previewBaseline(points, window, baseline) {
  const inside = points.filter(([t]) => t >= window.start && t < window.end);
  if (!inside.length) return null;
  let b0 = 0, b1 = 0;
  if (baseline === "vertical-drop") {
    b0 = b1 = Math.min(...inside.map((p) => p[1]));
  } else if (baseline === "valley-to-valley") {
    b0 = inside[0][1];
    b1 = inside[inside.length - 1][1];
  } else if (baseline === "percentile") {
    const span = window.end - window.start;
    const values = points
      .filter(([t]) => t >= window.start - span && t < window.end + span)
      .map((p) => p[1])
      .sort((a, b) => a - b);
    b0 = b1 = values[Math.floor(values.length / 20)] ?? 0;
  }
  return { b0, b1 };
}

export const sameWindow = (a, b) =>
  !!a && !!b && Math.abs(a.start - b.start) < 1e-6 &&
  Math.abs(a.end - b.end) < 1e-6;
