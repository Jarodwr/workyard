export type GraphNodeInput = {
  id: string;
  key: string;
  title: string;
  kind: "card" | "question";
  detail: string;
  resolved: boolean;
  phase_id?: string;
  cancelled?: boolean;
  context?: boolean;
};

export type GraphEdgeInput = {
  id: string;
  source: string; // Prerequisite
  target: string; // Dependent card
  resolved: boolean;
  scope?: "start" | "finish";
};

export type LayoutNode = GraphNodeInput & {
  level: number;
  x: number;
  y: number;
  impact: number; // Distinct downstream cards held by open links
  chokepoint: boolean;
  longestChain: boolean;
};

export type LayoutEdge = GraphEdgeInput & { longestChain: boolean };

export type DependencyLayout = {
  nodes: LayoutNode[];
  edges: LayoutEdge[];
  width: number;
  height: number;
  levels: number;
  longestChainLength: number;
  chokepoints: number;
  openEdges: number;
  phaseBands?: { id: string; name: string; description?: string; status: string; x: number; width: number }[];
};

export const GRAPH_NODE_WIDTH = 236;
export const GRAPH_NODE_HEIGHT = 96;
const COLUMN_SPAN = 340;
const ROW_SPAN = 124;
const TOP = 112;

export function questionOnlyBlockerCounts(
  nodes: GraphNodeInput[],
  edges: GraphEdgeInput[],
): Map<string, number> {
  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const unresolvedByCard = new Map<string, GraphEdgeInput[]>();
  for (const edge of edges) {
    if (edge.resolved) continue;
    const incoming = unresolvedByCard.get(edge.target) ?? [];
    incoming.push(edge);
    unresolvedByCard.set(edge.target, incoming);
  }

  const counts = new Map<string, number>();
  for (const card of nodes) {
    if (card.kind !== "card" || card.resolved || card.cancelled) continue;
    const questionIds = new Set<string>();
    let hasCardBlocker = false;
    for (const edge of unresolvedByCard.get(card.id) ?? []) {
      const prerequisite = nodeById.get(edge.source);
      if (prerequisite?.kind === "question" && !prerequisite.resolved) {
        questionIds.add(prerequisite.id);
      } else {
        // Unknown prerequisites should not make a card appear question-only.
        hasCardBlocker = true;
      }
    }
    if (hasCardBlocker) continue;
    for (const id of questionIds) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return counts;
}

export function buildDependencyLayout(
  nodeInputs: GraphNodeInput[],
  edgeInputs: GraphEdgeInput[],
): DependencyLayout {
  const allNodes = new Map(nodeInputs.map((node) => [node.id, node]));
  const edges = edgeInputs.filter(
    (edge) => allNodes.has(edge.source) && allNodes.has(edge.target),
  );
  const nodes = nodeInputs;
  const incoming = new Map(nodes.map((node) => [node.id, [] as GraphEdgeInput[]]));
  const outgoing = new Map(nodes.map((node) => [node.id, [] as GraphEdgeInput[]]));
  for (const edge of edges) {
    incoming.get(edge.target)?.push(edge);
    outgoing.get(edge.source)?.push(edge);
  }

  const indegree = new Map(nodes.map((node) => [node.id, incoming.get(node.id)?.length ?? 0]));
  const level = new Map(nodes.map((node) => [node.id, 0]));
  const queue = nodes.filter((node) => indegree.get(node.id) === 0).map((node) => node.id);
  const order: string[] = [];
  while (queue.length) {
    const id = queue.shift()!;
    order.push(id);
    for (const edge of outgoing.get(id) ?? []) {
      level.set(edge.target, Math.max(level.get(edge.target) ?? 0, (level.get(id) ?? 0) + 1));
      const next = (indegree.get(edge.target) ?? 0) - 1;
      indegree.set(edge.target, next);
      if (next === 0) queue.push(edge.target);
    }
  }
  // Core rejects cycles. This fallback keeps a readable view if an older DB is corrupt.
  for (const node of nodes) if (!order.includes(node.id)) order.push(node.id);

  const reach = new Map<string, Set<string>>();
  const openReach = new Map<string, Set<string>>();
  const tail = new Map<string, number>();
  for (const id of [...order].reverse()) {
    const downstream = new Set<string>();
    const waitingDownstream = new Set<string>();
    let longest = 1;
    for (const edge of outgoing.get(id) ?? []) {
      if (allNodes.get(edge.target)?.kind === "card") downstream.add(edge.target);
      for (const descendant of reach.get(edge.target) ?? []) downstream.add(descendant);
      if (!edge.resolved) {
        if (allNodes.get(edge.target)?.kind === "card") waitingDownstream.add(edge.target);
        for (const descendant of openReach.get(edge.target) ?? []) waitingDownstream.add(descendant);
      }
      longest = Math.max(longest, 1 + (tail.get(edge.target) ?? 1));
    }
    reach.set(id, downstream);
    openReach.set(id, waitingDownstream);
    tail.set(id, longest);
  }

  const ranked = [...nodes]
    .filter((node) => (openReach.get(node.id)?.size ?? 0) >= 2)
    .sort((a, b) => (openReach.get(b.id)?.size ?? 0) - (openReach.get(a.id)?.size ?? 0) || a.key.localeCompare(b.key));
  const chokepoints = new Set(ranked.slice(0, 3).map((node) => node.id));

  const roots = nodes.filter((node) => (incoming.get(node.id)?.length ?? 0) === 0);
  roots.sort((a, b) => (tail.get(b.id) ?? 1) - (tail.get(a.id) ?? 1) || a.key.localeCompare(b.key));
  const longestNodes = new Set<string>();
  const longestEdges = new Set<string>();
  let cursor = roots[0]?.id;
  while (cursor && !longestNodes.has(cursor)) {
    longestNodes.add(cursor);
    const next = [...(outgoing.get(cursor) ?? [])].sort(
      (a, b) => (tail.get(b.target) ?? 1) - (tail.get(a.target) ?? 1) || a.id.localeCompare(b.id),
    )[0];
    if (!next) break;
    longestEdges.add(next.id);
    cursor = next.target;
  }

  const levels = nodes.length ? Math.max(...nodes.map((node) => level.get(node.id) ?? 0)) + 1 : 0;
  const groups = Array.from({ length: levels }, (_, index) =>
    nodes.filter((node) => level.get(node.id) === index),
  );
  const row = new Map<string, number>();
  for (const [index, group] of groups.entries()) {
    group.sort((a, b) => {
      const center = (id: string) => {
        const parentRows = (incoming.get(id) ?? [])
          .map((edge) => row.get(edge.source))
          .filter((value): value is number => value !== undefined);
        return parentRows.length ? parentRows.reduce((sum, value) => sum + value, 0) / parentRows.length : 0;
      };
      return index === 0
        ? (reach.get(b.id)?.size ?? 0) - (reach.get(a.id)?.size ?? 0) || a.key.localeCompare(b.key)
        : center(a.id) - center(b.id) || (reach.get(b.id)?.size ?? 0) - (reach.get(a.id)?.size ?? 0) || a.key.localeCompare(b.key);
    });
    group.forEach((node, position) => row.set(node.id, position));
  }
  const maxRows = Math.max(1, ...groups.map((group) => group.length));
  const laidOut = groups.flatMap((group, column) =>
    group.map((node, position) => ({
      ...node,
      level: column,
      x: 24 + column * COLUMN_SPAN,
      y: TOP + ((maxRows - group.length) * ROW_SPAN) / 2 + position * ROW_SPAN,
      impact: openReach.get(node.id)?.size ?? 0,
      chokepoint: chokepoints.has(node.id),
      longestChain: longestNodes.has(node.id),
    })),
  );

  return {
    nodes: laidOut,
    edges: edges.map((edge) => ({ ...edge, longestChain: longestEdges.has(edge.id) })),
    width: levels ? 48 + (levels - 1) * COLUMN_SPAN + GRAPH_NODE_WIDTH : 0,
    height: TOP + maxRows * ROW_SPAN + 24,
    levels,
    longestChainLength: longestNodes.size,
    chokepoints: chokepoints.size,
    openEdges: edges.filter((edge) => !edge.resolved).length,
  };
}

export function buildPhaseDependencyLayout(
  nodeInputs: GraphNodeInput[],
  edgeInputs: GraphEdgeInput[],
  phases: { id: string; name: string; description?: string; status: string }[],
  metricNodes: GraphNodeInput[] = nodeInputs,
  metricEdges: GraphEdgeInput[] = edgeInputs,
): DependencyLayout {
  const global = buildDependencyLayout(nodeInputs, edgeInputs);
  const full = buildDependencyLayout(metricNodes, metricEdges);
  const metrics = new Map(full.nodes.map((node) => [node.id, node]));
  const fullEdges = new Map(full.edges.map((edge) => [edge.id, edge]));
  const nodes: LayoutNode[] = [];
  const phaseBands: NonNullable<DependencyLayout["phaseBands"]> = [];
  let offset = 0;
  let height = 220;
  let levelOffset = 0;
  for (const phase of phases) {
    const members = nodeInputs.filter((node) => node.phase_id === phase.id);
    const ids = new Set(members.map((node) => node.id));
    const internal = edgeInputs.filter((edge) => ids.has(edge.source) && ids.has(edge.target));
    const group = buildDependencyLayout(members, internal);
    const width = Math.max(300, group.width + 24);
    phaseBands.push({ ...phase, x: offset, width });
    for (const node of group.nodes) {
      const metric = metrics.get(node.id);
      nodes.push({ ...node, x: node.x + offset, level: node.level + levelOffset,
        impact: metric?.impact ?? 0, chokepoint: metric?.chokepoint ?? false,
        longestChain: metric?.longestChain ?? false });
    }
    height = Math.max(height, group.height);
    offset += width;
    levelOffset += Math.max(1, group.levels);
  }
  return { ...global, nodes, edges: global.edges.map((edge)=>({...edge,longestChain:fullEdges.get(edge.id)?.longestChain ?? false})), width: offset, height, levels: 0, phaseBands, longestChainLength:full.longestChainLength, chokepoints:nodes.filter((node)=>node.chokepoint).length };
}

export function traceDependencies(layout: DependencyLayout, selectedId: string) {
  const upstream = new Set<string>();
  const downstream = new Set<string>();
  const visit = (start: string, direction: "upstream" | "downstream", found: Set<string>) => {
    const queue = [start];
    while (queue.length) {
      const id = queue.shift()!;
      for (const edge of layout.edges) {
        const next = direction === "upstream"
          ? edge.target === id ? edge.source : null
          : edge.source === id ? edge.target : null;
        if (next && next !== start && !found.has(next)) {
          found.add(next);
          queue.push(next);
        }
      }
    }
  };
  visit(selectedId, "upstream", upstream);
  visit(selectedId, "downstream", downstream);
  return { upstream, downstream, related: new Set([selectedId, ...upstream, ...downstream]) };
}

// ---------------------------------------------------------------------------------------------
// Wire routing. Wires are orthogonal: leave the prerequisite's right side, run in the gutter
// beside its column, and arrive at the dependent's left side. A wire whose straight run would
// cross another card travels along a free corridor between rows instead. Parallel wires get
// their own track so unrelated wires never share a line; wires from one card (or into one card)
// share a trunk, so they read as a branch or a merge rather than as overlapping lines.

export type Point = [number, number];
/** `arrowhead` is false for wires that merge into another wire's arrival: they end at the trunk. */
export type RoutedEdge = { d: string; points: Point[]; arrowhead: boolean };

/** Zoom steps offered by the graph view. */
export const GRAPH_ZOOM_LEVELS = [0.5, 0.75, 1, 1.5] as const;
/** Height in screen pixels of the sticky phase header; the scaled graph sits below it. */
export const GRAPH_HEADER_HEIGHT = 56;

/** Screen-space frame of the graph at a zoom: the header takes its own rows, the graph scales below. */
export function graphFrame(layout: Pick<DependencyLayout, "width" | "height">, zoom: number) {
  return {
    width: layout.width * zoom,
    contentTop: GRAPH_HEADER_HEIGHT,
    height: GRAPH_HEADER_HEIGHT + layout.height * zoom,
  };
}

const CLEARANCE = 6; // space kept between a wire and a card
const TRACK = 5; // distance between parallel wires
const GUTTER_INSET = 14; // first track's distance from a card's side
const ARRIVAL_RUN = 34; // straight run into a card, long enough for the arrowhead to sit on it
const ARROW_GAP = 3; // the arrow tip's distance from the card's edge
const ARROW_HALF = 5; // half the arrowhead's length: a wire ends at the middle of its arrowhead
const MAX_ROW_GAP_TRACKS = 3; // tracks that fit between two rows without touching a card
const MAX_OPEN_TRACKS = 12; // tracks in a corridor with nothing beside it
const TOP_BAND_PENALTY = 60; // corridors above the first row sit under the sticky phase header

/** Does a horizontal run at `y` between `xa` and `xb` touch a card other than `skip`? */
function runHitsCard(nodes: LayoutNode[], xa: number, xb: number, y: number, skip: string): boolean {
  const lo = Math.min(xa, xb);
  const hi = Math.max(xa, xb);
  return nodes.some(
    (node) =>
      node.id !== skip &&
      node.x - CLEARANCE < hi && node.x + GRAPH_NODE_WIDTH + CLEARANCE > lo &&
      node.y - CLEARANCE < y && node.y + GRAPH_NODE_HEIGHT + CLEARANCE > y,
  );
}

/** Offsets 0, +TRACK, -TRACK, +2*TRACK, ... for the n-th wire sharing a corridor. */
function trackOffset(n: number): number {
  return n === 0 ? 0 : (n % 2 === 1 ? 1 : -1) * Math.ceil(n / 2) * TRACK;
}

/** Give each interval the lowest track number not used by an overlapping interval. */
function colourIntervals<T extends { start: number; end: number }>(items: T[]): Map<T, number> {
  const trackEnds: number[] = [];
  const result = new Map<T, number>();
  for (const item of [...items].sort((a, b) => a.start - b.start || a.end - b.end)) {
    let track = trackEnds.findIndex((end) => end < item.start - 4);
    if (track < 0) track = trackEnds.length;
    trackEnds[track] = item.end;
    result.set(item, track);
  }
  return result;
}

function roundedPath(raw: Point[], radius = 6): string {
  const points = raw.filter((p, i) => i === 0 || p[0] !== raw[i - 1][0] || p[1] !== raw[i - 1][1]);
  let d = `M ${points[0][0]} ${points[0][1]}`;
  for (let i = 1; i < points.length - 1; i++) {
    const [px, py] = points[i - 1];
    const [cx, cy] = points[i];
    const [nx, ny] = points[i + 1];
    const r = Math.min(radius, Math.hypot(cx - px, cy - py) / 2, Math.hypot(nx - cx, ny - cy) / 2);
    d += ` L ${cx - Math.sign(cx - px) * r} ${cy - Math.sign(cy - py) * r}`;
    d += ` Q ${cx} ${cy} ${cx + Math.sign(nx - cx) * r} ${cy + Math.sign(ny - cy) * r}`;
  }
  const last = points[points.length - 1];
  return `${d} L ${last[0]} ${last[1]}`;
}

export function routeDependencyEdges(nodes: LayoutNode[], edges: GraphEdgeInput[]): Map<string, RoutedEdge> {
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const minTop = nodes.length ? Math.min(...nodes.map((n) => n.y)) : 0;
  const corridorYs = new Set<number>();
  for (const node of nodes) {
    corridorYs.add(node.y - (ROW_SPAN - GRAPH_NODE_HEIGHT) / 2);
    corridorYs.add(node.y + GRAPH_NODE_HEIGHT + (ROW_SPAN - GRAPH_NODE_HEIGHT) / 2);
  }
  corridorYs.add(Math.max(64, minTop - 24));

  type Route = {
    edge: GraphEdgeInput; from: LayoutNode; to: LayoutNode;
    x1: number; y1: number; x2: number; y2: number;
    corridor: number | null; // y of the corridor run, or null for a direct run
  };
  const routes: Route[] = [];
  const occupancy = new Map<number, { start: number; end: number }[]>();
  for (const edge of [...edges].sort((a, b) => a.id.localeCompare(b.id))) {
    const from = byId.get(edge.source);
    const to = byId.get(edge.target);
    if (!from || !to) continue;
    const x1 = from.x + GRAPH_NODE_WIDTH;
    const y1 = from.y + GRAPH_NODE_HEIGHT / 2;
    const x2 = to.x;
    const y2 = to.y + GRAPH_NODE_HEIGHT / 2;
    // The straight run at the source's height, out to the gutter beside the dependent.
    const directClear = !runHitsCard(nodes, x1, x2 - ARRIVAL_RUN, y1, from.id);
    let corridor: number | null = null;
    if (!directClear) {
      // Cheapest clear corridor that still has a free track: the gap between rows only fits a few.
      const interval = { start: x1, end: x2 };
      let best = Infinity;
      for (const y of corridorYs) {
        if (runHitsCard(nodes, x1 + GUTTER_INSET, x2 - ARRIVAL_RUN, y, "")) continue;
        const cost = Math.abs(y - y1) + Math.abs(y - y2) + (y < minTop - 8 ? TOP_BAND_PENALTY : 0);
        if (cost >= best) continue;
        const crowd = [...(occupancy.get(y) ?? []), interval];
        const tracks = Math.max(...colourIntervals(crowd).values()) + 1;
        if (tracks > (y < minTop - 8 ? MAX_OPEN_TRACKS : MAX_ROW_GAP_TRACKS)) continue;
        best = cost; corridor = y;
      }
      corridor ??= Math.max(64, minTop - 24); // nothing clear: take the top band regardless
      occupancy.set(corridor, [...(occupancy.get(corridor) ?? []), interval]);
    }
    routes.push({ edge, from, to, x1, y1, x2, y2, corridor });
  }

  // Parallel corridor runs: spread wires that share a corridor and overlap horizontally.
  const corridorY = new Map<Route, number>();
  const byCorridor = new Map<number, Route[]>();
  for (const route of routes) {
    if (route.corridor === null) continue;
    byCorridor.set(route.corridor, [...(byCorridor.get(route.corridor) ?? []), route]);
  }
  for (const [y, group] of byCorridor) {
    const items = group.map((route) => ({ route, start: route.x1, end: route.x2 }));
    for (const [item, track] of colourIntervals(items)) corridorY.set(item.route, y + trackOffset(track));
  }

  // Vertical runs live in the gutters beside columns. Wires from one card share a trunk in its
  // right gutter, wires into one card share one in its left gutter; everything else gets a track.
  // `runYs` are the heights at which wires arrive at a trunk from the left; `endY` is the height
  // an "in" trunk's wires leave it towards their card.
  type Trunk = { key: string; gutter: number; start: number; end: number; side: "out" | "in"; runYs: Set<number>; endY: number };
  const trunks = new Map<string, Trunk>();
  const trunkFor = (key: string, gutter: number, side: "out" | "in", ya: number, yb: number, runY: number, endY: number) => {
    const found = trunks.get(key);
    const start = Math.min(ya, yb);
    const end = Math.max(ya, yb);
    if (found) { found.start = Math.min(found.start, start); found.end = Math.max(found.end, end); found.runYs.add(runY); }
    else trunks.set(key, { key, gutter, side, start, end, runYs: new Set([runY]), endY });
    return key;
  };
  const keys = new Map<Route, { out?: string; in?: string }>();
  for (const route of routes) {
    const yc = corridorY.get(route) ?? route.y1;
    const used: { out?: string; in?: string } = {};
    if (route.corridor !== null && yc !== route.y1) used.out = trunkFor(`out:${route.from.id}:${route.x1}`, route.x1, "out", route.y1, yc, route.y1, yc);
    const lastFrom = route.corridor !== null ? yc : route.y1;
    if (lastFrom !== route.y2) used.in = trunkFor(`in:${route.to.id}:${route.x2}`, route.x2, "in", lastFrom, route.y2, lastFrom, route.y2);
    keys.set(route, used);
  }
  const trackOf = new Map<string, number>();
  const byGutter = new Map<string, Trunk[]>();
  for (const trunk of trunks.values()) {
    const id = `${trunk.side}:${trunk.gutter}`;
    byGutter.set(id, [...(byGutter.get(id) ?? []), trunk]);
  }
  for (const group of byGutter.values()) {
    // A wire running at height h into trunk A must stay clear of the last stretch into any card
    // centred at h, and that stretch starts at its own trunk: so A has to sit further left.
    const mustBeLeftOf = (a: Trunk, b: Trunk) => a !== b && a.side === "in" && b.side === "in" && a.runYs.has(b.endY);
    const order: Trunk[] = [];
    const state = new Map<Trunk, "visiting" | "done">();
    const visit = (trunk: Trunk) => {
      if (state.has(trunk)) return; // already placed, or part of a cycle: ignore that constraint
      state.set(trunk, "visiting");
      for (const other of group) if (mustBeLeftOf(trunk, other)) visit(other);
      state.set(trunk, "done");
      order.push(trunk);
    };
    for (const trunk of [...group].sort((a, b) => a.start - b.start)) visit(trunk);
    const placed: Trunk[] = [];
    for (const trunk of order) {
      let track = Math.max(0, ...group.filter((other) => mustBeLeftOf(trunk, other) && trackOf.has(other.key)).map((other) => trackOf.get(other.key)! + 1));
      const taken = (t: number) => placed.some((other) => trackOf.get(other.key) === t && other.start - 4 <= trunk.end && other.end + 4 >= trunk.start);
      while (taken(track)) track++;
      trackOf.set(trunk.key, track);
      placed.push(trunk);
    }
  }

  const result = new Map<string, RoutedEdge>();
  const arrivalX = (route: Route) => {
    const key = keys.get(route)!.in;
    return route.x2 - ARRIVAL_RUN - (key ? trackOf.get(key)! * TRACK : 0);
  };
  // Every wire into a card overlaps on its last run, so only one of them draws the arrowhead:
  // the one reaching furthest left (ties by id). The others end where they join that run.
  const arrowOwner = new Map<string, Route>();
  for (const route of routes) {
    const best = arrowOwner.get(route.to.id);
    if (!best || arrivalX(route) < arrivalX(best)) arrowOwner.set(route.to.id, route);
  }
  for (const route of routes) {
    const used = keys.get(route)!;
    const yc = corridorY.get(route) ?? route.y1;
    const xIn = arrivalX(route);
    const arrowhead = arrowOwner.get(route.to.id) === route;
    const end: Point = arrowhead ? [route.x2 - ARROW_GAP - ARROW_HALF, route.y2] : [xIn, route.y2];
    let points: Point[];
    if (route.corridor === null) {
      points = [[route.x1, route.y1], [xIn, route.y1], [xIn, route.y2], end];
    } else {
      const xOut = route.x1 + GUTTER_INSET + (used.out ? trackOf.get(used.out)! * TRACK : 0);
      points = [[route.x1, route.y1], [xOut, route.y1], [xOut, yc], [xIn, yc], [xIn, route.y2], end];
    }
    result.set(route.edge.id, { d: roundedPath(points), points, arrowhead });
  }
  return result;
}
