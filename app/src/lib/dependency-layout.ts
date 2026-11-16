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
