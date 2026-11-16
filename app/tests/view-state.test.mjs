import assert from "node:assert/strict";
import test from "node:test";
import { loadViewState, saveViewState, pickZoom, pickPhaseFilter, restoreTarget } from "../src/lib/view-state.ts";

const memory = () => {
  const data = new Map();
  return { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v), data };
};

test("saving merges slices and round-trips per scope", () => {
  const s = memory();
  saveViewState("/db/a", { projectId: "p1", boardKey: "B-1", section: "kanban" }, s);
  saveViewState("/db/a", { zoom: 1.5 }, s);
  saveViewState("/db/b", { projectId: "other" }, s);
  assert.deepEqual(loadViewState("/db/a", s), { projectId: "p1", boardKey: "B-1", section: "kanban", zoom: 1.5 });
  assert.deepEqual(loadViewState("/db/b", s), { projectId: "other" });
});

test("corrupt or hostile storage reads as nothing saved", () => {
  const s = memory();
  s.setItem("workyard.view:x", "{not json");
  assert.deepEqual(loadViewState("x", s), {});
  s.setItem("workyard.view:x", JSON.stringify({ zoom: "big", section: "nope", boardKey: 4, agentsOpen: 1 }));
  assert.deepEqual(loadViewState("x", s), { boardKey: null });
  const throwing = { getItem() { throw new Error("denied"); }, setItem() { throw new Error("full"); } };
  assert.deepEqual(loadViewState("x", throwing), {});
  assert.doesNotThrow(() => saveViewState("x", { zoom: 1 }, throwing));
  assert.deepEqual(loadViewState("x", null), {});
});

test("zoom and phase filter fall back when the saved value is not offered", () => {
  assert.equal(pickZoom(1.5, [0.5, 1, 1.5], 1), 1.5);
  assert.equal(pickZoom(3, [0.5, 1, 1.5], 1), 1);
  assert.equal(pickZoom(undefined, [0.5, 1, 1.5], 1), 1);
  assert.equal(pickPhaseFilter("ph2", ["ph1", "ph2"]), "ph2");
  assert.equal(pickPhaseFilter("gone", ["ph1"]), "all");
  assert.equal(pickPhaseFilter("all", ["ph1"]), "all");
});

test("restore falls back one level when the board or project no longer exists", () => {
  const boards = [{ key: "B-1", project_id: "p1" }, { key: "B-2", project_id: null }];
  const U = "__unassigned__";
  assert.deepEqual(restoreTarget({ projectId: "p1", boardKey: "B-1" }, ["p1"], boards, U), { projectId: "p1", boardKey: "B-1" });
  assert.deepEqual(restoreTarget({ projectId: "p1", boardKey: "B-9" }, ["p1"], boards, U), { projectId: "p1", boardKey: null });
  assert.deepEqual(restoreTarget({ projectId: "p1", boardKey: "B-2" }, ["p1"], boards, U), { projectId: "p1", boardKey: null });
  assert.deepEqual(restoreTarget({ projectId: "gone", boardKey: "B-1" }, ["p1"], boards, U), { projectId: null, boardKey: null });
  assert.deepEqual(restoreTarget({ projectId: U, boardKey: "B-2" }, ["p1"], boards, U), { projectId: U, boardKey: "B-2" });
  assert.deepEqual(restoreTarget({}, ["p1"], boards, U), { projectId: null, boardKey: null });
});
