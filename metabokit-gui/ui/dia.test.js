import { parseWindows } from "./dia.js";
function equal(a, b) {
  if (JSON.stringify(a) !== JSON.stringify(b)) {
    throw new Error(`${JSON.stringify(a)} != ${JSON.stringify(b)}`);
  }
}
Deno.test("DIA windows retain order and overlapping boundaries", () => {
  equal(parseWindows("200, 480\n479,496\n495 511"), [[200, 480], [479, 496], [
    495,
    511,
  ]]);
});
Deno.test("DIA rejects malformed, reversed and nonfinite windows", () => {
  for (
    const input of [
      "",
      "1",
      "1,2,3",
      "2,1",
      "1,1",
      "-1,2",
      "NaN,2",
      "1,Infinity",
    ]
  ) {
    let failed = false;
    try {
      parseWindows(input);
    } catch {
      failed = true;
    }
    if (!failed) throw new Error(`Accepted invalid input ${input}`);
  }
});

import {
  acquisitionTime,
  drawnWindow,
  includedScans,
  moveEdge,
  nudgeEdge,
  previewBaseline,
  sequenceAxis,
  sequencePlan,
} from "./dia-integration.js";
const close = (a, b) => {
  if (Math.abs(a - b) > 1e-9) throw new Error(`${a} != ${b}`);
};
Deno.test("acquisition times ignore the local time zone and reject invalid dates", () => {
  equal(acquisitionTime("2025-02-10T16:55:21Z"), Date.UTC(2025, 1, 10, 16, 55, 21));
  equal(acquisitionTime("2025-02-10 16:55:21"), Date.UTC(2025, 1, 10, 16, 55, 21));
  equal(acquisitionTime("2025-02-10T16:55:21+02:00"), Date.UTC(2025, 1, 10, 14, 55, 21));
  for (const bad of ["", null, "2025-02-30T00:00:00Z", "yesterday"]) {
    equal(acquisitionTime(bad), null);
  }
});
Deno.test("sequence axis falls back to run order for missing or duplicate times", () => {
  const t = ["2025-02-10T18:00:00Z", "2025-02-10T16:00:00Z"];
  equal(sequenceAxis(t).timed, true);
  equal(sequenceAxis([t[0], null]), { timed: false, positions: [0, 1] });
  equal(sequenceAxis([t[0], t[0]]).timed, false);
});
Deno.test("sequence plan interpolates between references and holds outside them", () => {
  // Uneven acquisition gaps: interpolation follows time, not sample number.
  const positions = [0, 10, 40, 100, 130];
  const windows = positions.map(() => ({ start: 0, end: 1 }));
  windows[1] = { start: 1, end: 2 };
  windows[3] = { start: 4, end: 5.5 };
  const plan = sequencePlan(positions, windows, [3, 1]);
  equal(plan.map((p) => p.sample), [0, 2, 4]);
  equal([plan[0].start, plan[0].end], [1, 2]); // before the first reference
  close(plan[1].start, 1 + (4 - 1) / 3); // 40 is a third of the way from 10 to 100
  close(plan[1].end, 2 + (5.5 - 2) / 3);
  equal([plan[2].start, plan[2].end], [4, 5.5]); // after the last
});
Deno.test("one reference gives every sample its window; none gives no plan", () => {
  const plan = sequencePlan([0, 1, 2], [{ start: 1, end: 2 }, null, null], [0]);
  equal(plan.map((p) => [p.sample, p.start, p.end]), [[1, 1, 2], [2, 1, 2]]);
  equal(sequencePlan([0, 1], [null, null], []), []);
});
Deno.test("edges snap between scans and always keep two scans", () => {
  const times = [1, 1.1, 1.2, 1.3, 1.4, 1.5];
  const w = { start: 1.15, end: 1.35 };
  equal(includedScans(times, w), [2, 3]);
  close(moveEdge(times, w, "start", 1.04).start, 0.95);
  close(moveEdge(times, w, "start", 1.49).start, 1.15); // keeps scans 1.2 and 1.3
  close(moveEdge(times, w, "end", 1.0).end, 1.35);
  close(nudgeEdge(times, w, "end", 1).end, 1.45);
  close(nudgeEdge(times, w, "start", -5).start, 0.95);
  const d = drawnWindow(times, 1.42, 1.18);
  close(d.start, 1.15);
  close(d.end, 1.45);
  equal(includedScans(times, drawnWindow(times, 1.5, 1.5)).length, 2);
});
Deno.test("preview baselines match the native definitions", () => {
  const pts = [[0, 2], [1, 4], [2, 12], [3, 6], [4, 2]];
  const w = { start: 1, end: 4 };
  equal(previewBaseline(pts, w, "none"), { b0: 0, b1: 0 });
  equal(previewBaseline(pts, w, "vertical-drop"), { b0: 4, b1: 4 });
  equal(previewBaseline(pts, w, "valley-to-valley"), { b0: 4, b1: 6 });
  equal(previewBaseline(pts, w, "percentile"), { b0: 2, b1: 2 });
  equal(previewBaseline(pts, { start: 9, end: 10 }, "none"), null);
});
