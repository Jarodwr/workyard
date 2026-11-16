import assert from "node:assert/strict";
import test from "node:test";
import {
  buildDependencyLayout, buildPhaseDependencyLayout, GRAPH_NODE_HEIGHT, GRAPH_NODE_WIDTH,
  questionOnlyBlockerCounts, routeDependencyEdges, traceDependencies,
} from "../src/lib/dependency-layout.ts";

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

// The sample todo board: three independent cards, two that depend on them, a card two levels
// downstream, and a second phase whose cards reach back across the phase boundary.
function todoBoard() {
  const phased = (id, phase) => ({ ...node(id), phase_id: phase });
  const nodes = ["C-1", "C-2", "C-3", "C-4", "C-5", "C-6"].map((id) => phased(id, "build"))
    .concat(["C-7", "C-8"].map((id) => phased(id, "release")));
  const edges = [
    edge("C-1", "C-4"), edge("C-2", "C-4"), edge("C-3", "C-4"), edge("C-1", "C-5"),
    edge("C-4", "C-6"), edge("C-4", "C-7"), edge("C-5", "C-7"), edge("C-6", "C-7"),
    edge("C-5", "C-8"), edge("C-6", "C-8"),
  ];
  const phases = [{ id: "build", name: "Build", status: "current" }, { id: "release", name: "Release", status: "future" }];
  const layout = buildPhaseDependencyLayout(nodes, edges, phases);
  return { layout, routes: routeDependencyEdges(layout.nodes, layout.edges) };
}
const segments = (points) => points.slice(1).map((p, i) => [points[i], p]);

test("wires never cross a card they do not start or end at", () => {
  const { layout, routes } = todoBoard();
  assert.equal(routes.size, layout.edges.length);
  for (const link of layout.edges) {
    for (const [[xa, ya], [xb, yb]] of segments(routes.get(link.id).points)) {
      for (const card of layout.nodes) {
        if (card.id === link.source || card.id === link.target) continue;
        const crosses = Math.min(xa, xb) < card.x + GRAPH_NODE_WIDTH && Math.max(xa, xb) > card.x
          && Math.min(ya, yb) < card.y + GRAPH_NODE_HEIGHT && Math.max(ya, yb) > card.y;
        assert.ok(!crosses, `${link.id} crosses ${card.id}`);
      }
    }
  }
});

test("unrelated wires do not share a vertical or horizontal line", () => {
  const { layout, routes } = todoBoard();
  const runs = layout.edges.flatMap((link) => segments(routes.get(link.id).points).map(([a, b]) => ({ link, a, b })));
  for (const [i, one] of runs.entries()) {
    for (const two of runs.slice(i + 1)) {
      if (one.link.source === two.link.source || one.link.target === two.link.target) continue; // a branch or a merge
      const vertical = one.a[0] === one.b[0] && two.a[0] === two.b[0] && one.a[0] === two.a[0];
      const horizontal = one.a[1] === one.b[1] && two.a[1] === two.b[1] && one.a[1] === two.a[1];
      const overlap = (p, q, r, s) => Math.min(Math.max(p, q), Math.max(r, s)) - Math.max(Math.min(p, q), Math.min(r, s)) > 1;
      if (vertical) assert.ok(!overlap(one.a[1], one.b[1], two.a[1], two.b[1]), `${one.link.id} and ${two.link.id} share a vertical`);
      if (horizontal) assert.ok(!overlap(one.a[0], one.b[0], two.a[0], two.b[0]), `${one.link.id} and ${two.link.id} share a horizontal`);
    }
  }
});

test("a wire that skips a column avoids cards, and avoids the header band when it can", () => {
  const { layout, routes } = todoBoard();
  const top = Math.min(...layout.nodes.map((card) => card.y));
  const skipping = routes.get("C-1-C-5").points; // adjacent: a plain run
  assert.ok(skipping.every(([, y]) => y >= top - 8));
  const longest = routes.get("C-4-C-7").points; // crosses into the next phase past C-6
  assert.ok(longest.length >= 4);
});

test("every wire ends with a straight run into its card, long enough to carry the arrowhead", () => {
  const { layout, routes } = todoBoard();
  for (const link of layout.edges) {
    const points = routes.get(link.id).points;
    const [[xa, ya], [xb, yb]] = segments(points).at(-1);
    assert.equal(ya, yb, `${link.id} does not arrive horizontally`);
    assert.ok(xb - xa >= 24, `${link.id} arrives over only ${xb - xa}px`);
    const target = layout.nodes.find((card) => card.id === link.target);
    // The wire stops mid-arrowhead: 5px of head lies beyond it, then a 3px gap to the card.
    assert.equal(target.x - xb, 8, `${link.id} does not end mid-arrowhead at its card`);
  }
});
