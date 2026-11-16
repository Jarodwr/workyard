/** Which tab the card detail modal opens on, and which tabs carry an attention badge. */
export const CARD_TABS = ["overview", "activity", "history", "code", "links", "report"] as const;
export type CardTab = (typeof CARD_TABS)[number];

export type TabBadge = { kind: "count" | "dot"; count: number; label: string };

/** The few card fields the tab logic reads, so it stays testable without the full API types. */
export type TabCardInput = {
  open_question_count: number;
  blocked?: boolean;
  blockers?: unknown[];
  review_gates?: { approved: boolean }[];
  session?: { paused?: boolean; held_by?: string | null; waiting_review?: boolean } | null;
};

export function isRunningSession(card: TabCardInput): boolean {
  const session = card.session;
  return !!session && !session.held_by && !session.paused;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** Badges per tab; a tab with nothing needing attention has no entry. */
export function tabBadges(card: TabCardInput): Partial<Record<CardTab, TabBadge>> {
  const badges: Partial<Record<CardTab, TabBadge>> = {};
  const questions = Math.max(0, card.open_question_count || 0);
  if (questions > 0) badges.links = { kind: "count", count: questions, label: plural(questions, "open question") };
  const reviews = (card.review_gates ?? []).filter((gate) => !gate.approved).length;
  if (reviews > 0) badges.code = { kind: "count", count: reviews, label: plural(reviews, "review waiting for approval") };
  if (isRunningSession(card)) badges.activity = { kind: "dot", count: 0, label: "agent running" };
  const blockers = card.blocked ? (card.blockers?.length ?? 0) : 0;
  if (blockers > 0) badges.overview = { kind: "count", count: blockers, label: plural(blockers, "blocker") };
  return badges;
}

/**
 * The tab to open on. A tab the user already chose for this card wins; otherwise a waiting
 * review or question takes you to where it is handled; otherwise Overview.
 */
export function initialTab(card: TabCardInput, remembered?: CardTab | null): CardTab {
  if (remembered && CARD_TABS.includes(remembered)) return remembered;
  const badges = tabBadges(card);
  if (badges.code) return "code";
  if (badges.links) return "links";
  return "overview";
}

/** Next tab for an arrow/Home/End key press, or null for any other key. */
export function nextTab(current: CardTab, key: string): CardTab | null {
  const index = CARD_TABS.indexOf(current);
  if (key === "ArrowRight" || key === "ArrowDown") return CARD_TABS[(index + 1) % CARD_TABS.length];
  if (key === "ArrowLeft" || key === "ArrowUp") return CARD_TABS[(index + CARD_TABS.length - 1) % CARD_TABS.length];
  if (key === "Home") return CARD_TABS[0];
  if (key === "End") return CARD_TABS[CARD_TABS.length - 1];
  return null;
}
