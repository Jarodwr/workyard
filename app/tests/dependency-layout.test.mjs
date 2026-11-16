import assert from "node:assert/strict";
import test from "node:test";
import { buildDependencyLayout, questionOnlyBlockerCounts, traceDependencies } from "../src/lib/dependency-layout.ts";

const node = (id, kind = "card") => ({
  id, key: id, title: id, kind, detail: "", resolved: false,
});
const edge = (source, target, resolved = false) => ({
  id: `${source}-${target}`, source, target, resolved,
});

test("layers branches and merges in dependency order", () => {
  const graph = buildDependencyLayout(
    [node("Q", "question"), node("A"), node("B"), node("C"), node("D"), node("E")],
    [edge("A", "B"), edge("B", "C"), edge("B", "D"), edge("C", "E"), edge("D", "E"), edge("Q", "D")],
  );
  const byId = new Map(graph.nodes.map((item) => [item.id, item]));
  assert.equal(graph.levels, 4);
  assert.equal(graph.longestChainLength, 4);
  assert.equal(byId.get("B").impact, 3);
  assert.equal(byId.get("E").level, 3);
  assert.ok(graph.edges.every((link) => byId.get(link.source).level < byId.get(link.target).level));
  const focus = traceDependencies(graph, "C");
  assert.deepEqual([...focus.upstream].sort(), ["A", "B"]);
  assert.deepEqual([...focus.downstream], ["E"]);
  assert.ok(!focus.related.has("D"));
});

test("completed prerequisites stop being current chokepoints", () => {
  const graph = buildDependencyLayout(
    [node("A"), node("B"), node("C"), node("D")],
    [edge("A", "B", true), edge("B", "C"), edge("B", "D")],
  );
  const byId = new Map(graph.nodes.map((item) => [item.id, item]));
  assert.equal(byId.get("A").impact, 0);
  assert.equal(byId.get("B").impact, 2);
  assert.equal(byId.get("B").chokepoint, true);
  assert.equal(graph.longestChainLength, 3);
});

test("highlights questions only when unfinished tasks have no unresolved task blockers", () => {
  const nodes = [node("Q1", "question"), node("Q2", "question"), node("A"), node("B"), node("C")];
  const questionsOnly = [edge("Q1", "A"), edge("Q2", "A"), edge("Q1", "B")];
  assert.deepEqual([...questionOnlyBlockerCounts(nodes, questionsOnly)], [["Q1", 2], ["Q2", 1]]);

  const cardBlocker = edge("C", "A");
  assert.deepEqual([...questionOnlyBlockerCounts(nodes, [...questionsOnly, cardBlocker])], [["Q1", 1]]);
  assert.deepEqual([...questionOnlyBlockerCounts(nodes, [...questionsOnly, { ...cardBlocker, resolved: true }])], [["Q1", 2], ["Q2", 1]]);

  const completed = nodes.map((item) => item.id === "B" ? { ...item, resolved: true } : item);
  assert.deepEqual([...questionOnlyBlockerCounts(completed, questionsOnly)], [["Q1", 1], ["Q2", 1]]);
  const cancelled = nodes.map((item) => item.id === "B" ? { ...item, cancelled: true } : item);
  assert.deepEqual([...questionOnlyBlockerCounts(cancelled, questionsOnly)], [["Q1", 1], ["Q2", 1]]);
});
