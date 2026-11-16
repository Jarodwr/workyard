import assert from "node:assert/strict";
import test from "node:test";
import {
  buildDependencyLayout, buildPhaseDependencyLayout, GRAPH_HEADER_HEIGHT, GRAPH_NODE_HEIGHT, GRAPH_NODE_WIDTH, GRAPH_ZOOM_LEVELS, graphFrame,
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
    if (!routes.get(link.id).arrowhead) continue; // merged wires end at the trunk
    const points = routes.get(link.id).points;
    const [[xa, ya], [xb, yb]] = segments(points).at(-1);
    assert.equal(ya, yb, `${link.id} does not arrive horizontally`);
    assert.ok(xb - xa >= 24, `${link.id} arrives over only ${xb - xa}px`);
    const target = layout.nodes.find((card) => card.id === link.target);
    // The wire stops mid-arrowhead: 5px of head lies beyond it, then a 3px gap to the card.
    if (routes.get(link.id).arrowhead) assert.equal(target.x - xb, 8, `${link.id} does not end mid-arrowhead at its card`);
  }
});

const noCrossings = (layout, routes) => {
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
};

test("wires merging into one card draw a single arrowhead; the rest end at the trunk", () => {
  const { layout, routes } = todoBoard();
  const incomingTo = new Map();
  for (const link of layout.edges) incomingTo.set(link.target, [...(incomingTo.get(link.target) ?? []), link]);
  for (const [target, links] of incomingTo) {
    const heads = links.filter((link) => routes.get(link.id).arrowhead);
    assert.equal(heads.length, 1, `${target} has ${heads.length} arrowheads`);
    const card = layout.nodes.find((item) => item.id === target);
    for (const link of links) {
      const [, [x, y]] = segments(routes.get(link.id).points).at(-1);
      if (routes.get(link.id).arrowhead) continue;
      assert.ok(x < card.x - 8, `${link.id} runs on to the arrowhead`);
      assert.equal(y, card.y + GRAPH_NODE_HEIGHT / 2);
    }
  }
  assert.ok(incomingTo.get("C-7").length >= 3);
});

function bigBoard() {
  const phases = ["p1", "p2", "p3"].map((id) => ({ id, name: id, status: "current" }));
  const nodes = Array.from({ length: 20 }, (_, i) => ({ ...node(`N${String(i).padStart(2, "0")}`), phase_id: phases[i < 7 ? 0 : i < 14 ? 1 : 2].id }));
  const edges = [];
  for (let i = 1; i < 20; i++) {
    edges.push(edge(nodes[Math.floor(i / 2) - (i > 12 ? 1 : 0)].id, nodes[i].id));
    if (i > 3) edges.push(edge(nodes[i - 3].id, nodes[i].id));
    if (i > 8 && i % 3 === 0) edges.push(edge(nodes[i - 8].id, nodes[i].id));
  }
  const unique = [...new Map(edges.map((link) => [link.id, link])).values()];
  const layout = buildPhaseDependencyLayout(nodes, unique, phases);
  return { layout, routes: routeDependencyEdges(layout.nodes, layout.edges) };
}

test("a 20-card board over three phases routes every wire clear of every card", () => {
  const { layout, routes } = bigBoard();
  assert.equal(layout.nodes.length, 20);
  assert.equal(routes.size, layout.edges.length);
  noCrossings(layout, routes);
  const merges = new Map();
  for (const link of layout.edges) if (routes.get(link.id).arrowhead) merges.set(link.target, (merges.get(link.target) ?? 0) + 1);
  assert.ok([...merges.values()].every((count) => count === 1));
});

test("at every zoom level the graph sits below the sticky header and wires stay inside it", () => {
  for (const { layout, routes } of [todoBoard(), bigBoard()]) {
    noCrossings(layout, routes);
    for (const zoom of GRAPH_ZOOM_LEVELS) {
      const frame = graphFrame(layout, zoom);
      assert.equal(frame.contentTop, GRAPH_HEADER_HEIGHT);
      for (const { points } of routes.values()) {
        for (const [x, y] of points) {
          assert.ok(frame.contentTop + y * zoom >= GRAPH_HEADER_HEIGHT, `wire under the header at ${zoom}`);
          assert.ok(frame.contentTop + y * zoom <= frame.height, `wire below the graph at ${zoom}`);
          assert.ok(x * zoom >= 0 && x * zoom <= frame.width, `wire outside the graph at ${zoom}`);
        }
      }
    }
  }
  assert.deepEqual([...GRAPH_ZOOM_LEVELS], [0.5, 0.75, 1, 1.5]);
});
