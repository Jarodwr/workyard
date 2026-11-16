import { useEffect, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import {
  ArrowRight,
  CircleHelp,
  Clock3,
  Link2,
  LockKeyhole,
  UserRound,
  X,
} from "lucide-react";
import type { Board, Card, CardChange, CardDetail as Detail, Question } from "@/api";
import { AgentLog } from "@/components/workspace/agent-log";
import { stateTone } from "@/components/workspace/agents-dock";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card as UiCard } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import { TabList, TabPanel, type TabItem } from "@/components/ui/tabs";
import { agentState } from "@/lib/agents";
import { initialTab, nextTab, tabBadges, type CardTab } from "@/lib/card-tabs";
import { cn } from "@/lib/utils";
import { Markdown } from "@/components/workspace/markdown";

const PREFIX = "card-detail";
const TAB_LABELS: Record<CardTab, string> = { overview: "Overview", activity: "Activity", history: "History", code: "Code", links: "Links", report: "Report" };
/** Last tab chosen per card, kept for the session. */
const rememberedTabs = new Map<string, CardTab>();

type Props = {
  card: Card | null;
  detail: Detail | null;
  board: Board | null;
  questions: Question[];
  onClose: () => void;
  onAddNote: (card: Card, body: string) => Promise<boolean>;
  onApproveChange: (change: CardChange) => Promise<boolean>;
  onOpenInTerminal: (card: Card) => Promise<boolean>;
  onReleaseHold: (card: Card) => Promise<boolean>;
};

export function CardDetail({
  card,
  detail,
  board,
  questions,
  onClose,
  onAddNote,
  onApproveChange,
  onOpenInTerminal,
  onReleaseHold,
}: Props) {
  const reduced = useReducedMotion();
  const [note, setNote] = useState("");
  useEffect(() => setNote(""), [card?.id]);
  const [tab, setTab] = useState<CardTab>("overview");
  const cardId = card?.id;
  // Open on the remembered tab, else the one with something waiting, else Overview; only when the card changes,
  // so a live refresh never moves you off the tab you are reading.
  useEffect(() => {
    if (card) setTab(initialTab(card, rememberedTabs.get(card.id)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cardId]);
  const chooseTab = (next: CardTab) => {
    if (cardId) rememberedTabs.set(cardId, next);
    setTab(next);
  };
  const badges = card ? tabBadges(card) : {};
  const tabItems: TabItem<CardTab>[] = (Object.keys(TAB_LABELS) as CardTab[]).map((id) => ({ id, label: TAB_LABELS[id], badge: badges[id] ?? null }));
  const state = card ? agentState(card) : null;
  const lane = card ? board?.lanes.find((item) => item.id === card.lane_id) : undefined;
  useEffect(() => {
    if (!card) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [card, onClose]);
  const labelForTarget = (id: string | null) => {
    if (!id) return "Unknown";
    const targetCard = board?.lanes
      .flatMap((lane) => lane.cards)
      .find((item) => item.id === id);
    if (targetCard) return `${targetCard.key} · ${targetCard.title}`;
    const question = questions.find((item) => item.id === id);
    return question ? `${question.key} · ${question.title}` : id;
  };

  return (
    <AnimatePresence>
      {card && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: reduced ? 0 : 0.2 }}
          onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
        >
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label={`Card ${card.key}`}
            className="flex h-[min(850px,calc(100dvh-2rem))] w-full max-w-2xl flex-col overflow-hidden rounded-xl border bg-background shadow-xl"
            initial={{ opacity: 0, scale: reduced ? 1 : 0.97 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: reduced ? 1 : 0.97 }}
            transition={{ duration: reduced ? 0 : 0.2 }}
          >
            <div className="border-b px-5 pb-3 pt-3">
              <div className="flex items-center justify-between gap-2">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="font-mono text-xs text-muted-foreground">
                    {card.key}
                  </span>
                  <Badge variant="secondary">{board?.phases.find((phase)=>phase.id===card.phase_id)?.name ?? "Phase"}</Badge>
                  {lane && <Badge variant="outline">{lane.name}</Badge>}
                  {card.cancelled_at && <Badge variant="outline">Cancelled</Badge>}
                  {card.blocking_impact > 0 && <Badge variant="outline">Holds up {card.blocking_impact}</Badge>}
                  {card.blocked && (
                    <Badge variant="destructive">
                      <LockKeyhole className="size-3" /> Blocked
                    </Badge>
                  )}
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Close card detail"
                  onClick={onClose}
                >
                  <X />
                </Button>
              </div>
              <h2 className="mt-2 text-lg font-semibold leading-snug tracking-tight">
                {card.title}
              </h2>
              <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1.5 text-xs">
                <span className="flex min-w-0 items-center gap-1.5 font-medium">
                  <UserRound className="size-3.5 shrink-0" />
                  <span className="truncate">{card.owner_actor || "Unclaimed"}</span>
                </span>
                {state && (
                  <span className={cn("inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[10px] font-medium", stateTone[state.kind])}>
                    {state.kind === "running" && <span className="agent-running-dot size-1.5 rounded-full bg-primary" aria-hidden />}
                    {state.label}
                  </span>
                )}
                <div className="ml-auto flex items-center gap-2">
                  {detail?.session?.held_by ? (
                    <Button size="sm" variant="outline" className="h-7 text-xs" onClick={() => void onReleaseHold(card)}>Release hold</Button>
                  ) : detail?.session?.runner === "claude" ? (
                    <Button size="sm" variant="outline" className="h-7 text-xs" title="Stops the running worker and opens this session in your terminal" onClick={() => void onOpenInTerminal(card)}>Open in terminal</Button>
                  ) : null}
                </div>
              </div>
              <div className="mt-2.5 flex items-center gap-3">
                <Progress value={card.progress} />
                <span className="font-mono text-xs tabular-nums text-muted-foreground">{card.progress}%</span>
              </div>
            </div>
            <TabList
              prefix={PREFIX}
              label="Card sections"
              items={tabItems}
              value={tab}
              onChange={chooseTab}
              next={nextTab}
            />

            <TabPanel prefix={PREFIX} id="overview" active={tab === "overview"}>
              {card.description
                ? <Markdown source={card.description} />
                : <p className="text-sm leading-relaxed text-muted-foreground">No description yet.</p>}
              {card.cancelled_at && <p className="mt-3 rounded border p-3 text-xs text-muted-foreground">Cancelled: {card.cancellation_reason}</p>}
              <div className="mt-4 grid grid-cols-1 gap-2 min-[420px]:grid-cols-2">
                <UiCard className="p-2.5 shadow-none">
                  <p className="text-[11px] text-muted-foreground">Owner</p>
                  <p className="mt-1 flex items-center gap-1.5 truncate text-xs font-medium">
                    <UserRound className="size-3.5" />{" "}
                    {card.owner_actor || "Unclaimed"}
                  </p>
                  {card.stale && <p className="mt-1.5 flex items-center gap-1 text-[11px] text-amber-700 dark:text-amber-300"><Clock3 className="size-3" /> Stale since {card.last_activity_at ? new Date(card.last_activity_at).toLocaleString() : "it was claimed"}</p>}
                </UiCard>
                <UiCard className="p-2.5 shadow-none">
                  <p className="text-[11px] text-muted-foreground">Agent type</p>
                  <p className="mt-1 text-xs font-semibold">
                    {card.agent_type || "Unspecified"}
                  </p>
                </UiCard>
              </div>
              <section className="mt-4 space-y-2">
                <h3 className="text-sm font-semibold">Status</h3>
                <p className="text-xs text-muted-foreground">{card.status_note || "No status update yet."}</p>
              </section>
              {card.blocked && (
                <UiCard className="mt-3 border-amber-500/25 bg-amber-500/5 p-3 shadow-none">
                  <p className="flex items-center gap-2 text-xs font-semibold text-amber-700 dark:text-amber-300">
                    <LockKeyhole className="size-4" /> Blocked by
                  </p>
                  <div className="mt-2 space-y-1">
                    {card.blockers.map((blocker) => (
                      <p key={blocker.id} className="text-xs leading-relaxed">
                        <span className="font-mono text-muted-foreground">
                          {blocker.key}
                        </span>{" "}
                        · {blocker.title}
                      </p>
                    ))}
                  </div>
                </UiCard>
              )}
              {!!card.cancelled_prerequisites?.length && (
                <section className="mt-3">
                  <p className="text-xs font-semibold text-muted-foreground">Cancelled prerequisites (no longer blocking)</p>
                  {card.cancelled_prerequisites.map((blocker) => (
                    <p key={blocker.id} className="mt-1 text-xs text-muted-foreground line-through">{blocker.key} · {blocker.title}</p>
                  ))}
                </section>
              )}
              {!card.can_finish && (!!card.finish_blockers?.length || card.review_gates?.some((gate) => !gate.approved)) && (
                <UiCard className="mt-3 border-indigo-500/25 bg-indigo-500/5 p-3 shadow-none">
                  <p className="text-xs font-semibold text-indigo-700 dark:text-indigo-300">Before this card can finish</p>
                  {card.finish_blockers?.map((blocker) => <p key={blocker.id} className="mt-1 text-xs">{blocker.key} · {blocker.title}</p>)}
                  {card.review_gates?.filter((gate) => !gate.approved).map((gate) => <p key={gate.change_id} className="mt-1 text-xs">Review approval for {gate.working_branch}{gate.commit_oid && ` at ${gate.commit_oid.slice(0, 8)}`}</p>)}
                </UiCard>
              )}
            </TabPanel>

            <TabPanel prefix={PREFIX} id="activity" active={tab === "activity"}>
              {detail?.session && (
                <UiCard className="p-3 shadow-none">
                  <p className="text-[11px] text-muted-foreground">Agent session</p>
                  <p className="mt-1 text-xs font-medium">{detail.session.runner} · {detail.session.held_by ? `Held by ${detail.session.held_by}` : detail.session.paused_at ? "Paused" : "Attached"}</p>
                  {detail.session.waiting_question_key && <p className="mt-1 text-xs text-muted-foreground">Waiting for {detail.session.waiting_question_key}</p>}
                  <p className="mt-1 text-xs text-muted-foreground">{card.workspace === "none" ? "No worktree (investigation)" : "Own git worktree"}</p>
                </UiCard>
              )}
              {card.session && state && (
                <section className="mt-3 space-y-1.5 first:mt-0" aria-label="Live agent">
                  <div className="flex items-center gap-2">
                    <h3 className="text-sm font-semibold">Live</h3>
                    <span className={cn("inline-flex items-center gap-1 rounded-full border px-1.5 py-px text-[10px] font-medium", stateTone[state.kind])}>
                      {state.kind === "running" && <span className="agent-running-dot size-1.5 rounded-full bg-primary" aria-hidden />}
                      {state.label}
                    </span>
                  </div>
                  {card.session.activity && state.kind === "running" && <p className="truncate text-xs text-muted-foreground" title={card.session.activity}>{card.session.activity}</p>}
                  <AgentLog cardKey={card.key} />
                </section>
              )}
              {(detail?.session || card.session) && <Separator className="my-4" />}
              <section className="space-y-2">
                <h3 className="text-sm font-semibold">Notes</h3>
                <p className="text-xs text-muted-foreground">Leave context for the agent without changing its ticket.</p>
                {detail?.notes.map((item) => (
                  <div key={item.id} className="rounded-lg border bg-card p-3 text-xs">
                    <Markdown source={item.body} className="text-xs" />
                    <p className="mt-2 text-muted-foreground">{item.actor} · {new Date(item.created_at).toLocaleString()}</p>
                  </div>
                ))}
                <label htmlFor="card-note" className="sr-only">Add a note</label>
                <textarea
                  id="card-note"
                  rows={3}
                  placeholder="Write a note for the agent…"
                  value={note}
                  onChange={(event) => setNote(event.target.value)}
                  className="w-full resize-y rounded-md border bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                />
                <Button size="sm" disabled={!note.trim()} onClick={async () => { if (await onAddNote(card, note.trim())) setNote(""); }}>Add note</Button>
              </section>
            </TabPanel>

            <TabPanel prefix={PREFIX} id="history" active={tab === "history"}>
              <section>
                <h3 className="mb-2 flex items-center gap-2 text-sm font-semibold">
                  <Clock3 className="size-4 text-muted-foreground" /> Recent
                  activity
                </h3>
                {detail?.events.length ? (
                  <div className="space-y-3 border-l pl-4">
                    {detail.events.slice(0, 20).map((event) => (
                      <div key={event.id} className="text-xs">
                        <p className="font-medium">
                          {event.type.replaceAll("_", " ")}
                        </p>
                        <p className="mt-0.5 text-muted-foreground">
                          {event.actor} · {new Date(event.ts).toLocaleString()}
                        </p>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="text-xs text-muted-foreground">
                    No activity yet.
                  </p>
                )}
              </section>
            </TabPanel>

            <TabPanel prefix={PREFIX} id="code" active={tab === "code"}>
              <section className="space-y-2">
                <h3 className="text-sm font-semibold">Code changes</h3>
                {detail?.changes?.length ? detail.changes.map((change) => <UiCard key={change.id} className="p-3 text-xs shadow-none">
                  <p className="break-all font-medium">{change.working_branch} <span className="text-muted-foreground">→ {change.target_branch}</span></p>
                  <p className="mt-1 text-muted-foreground">{change.commit_oid ? `Commit ${change.commit_oid.slice(0, 12)}` : "Commit not recorded"}{change.review_required ? ` · ${change.approved_commit_oid === change.commit_oid && change.commit_oid ? "Review approved" : "Review pending"}` : ""}</p>
                  {change.review_required && !!change.commit_oid && change.approved_commit_oid !== change.commit_oid && (
                    <Button size="sm" className="mt-2 h-7 text-xs" onClick={() => void onApproveChange(change)}>Approve this commit</Button>
                  )}
                </UiCard>) : (
                  <p className="text-xs text-muted-foreground">No code changes linked yet.</p>
                )}
              </section>
              {!!card.review_gates?.length && (
                <section className="mt-4 space-y-1">
                  <h3 className="text-sm font-semibold">Review gates</h3>
                  {card.review_gates.map((gate) => (
                    <p key={gate.change_id} className="text-xs text-muted-foreground">
                      {gate.working_branch}{gate.commit_oid && ` at ${gate.commit_oid.slice(0, 8)}`} · {gate.approved ? `Approved${gate.reviewer ? ` by ${gate.reviewer}` : ""}` : "Waiting for approval"}
                    </p>
                  ))}
                </section>
              )}
            </TabPanel>

            <TabPanel prefix={PREFIX} id="report" active={tab === "report"}>
              {detail?.report ? (
                <section className="space-y-2">
                  <Markdown source={detail.report.body} />
                  <p className="text-xs text-muted-foreground">{detail.report.actor} · {new Date(detail.report.updated_at).toLocaleString()} · revision {detail.report.version}</p>
                </section>
              ) : (
                <p className="text-xs text-muted-foreground">No report yet. An agent attaches one with <code className="font-mono">workyard task report {card.key} --file report.md</code>.</p>
              )}
            </TabPanel>

            <TabPanel prefix={PREFIX} id="links" active={tab === "links"}>
              <section className="space-y-2">
                <div className="flex items-center gap-2">
                  <Link2 className="size-4 text-muted-foreground" />
                  <h3 className="text-sm font-semibold">Dependencies</h3>
                </div>
                {detail?.dependencies.length ? (
                  detail.dependencies.map((dep) => {
                    const outbound = dep.from_card_id === card.id;
                    const ref = dep.to_card_id || dep.to_question_id;
                    return (
                      <div
                        key={dep.id}
                        className="flex items-center gap-2 rounded-lg border px-3 py-2 text-xs"
                      >
                        <ArrowRight className="size-3.5 shrink-0 text-muted-foreground" />
                        <span className="min-w-0 flex-1 truncate">
                          {outbound
                            ? `${dep.scope === "finish" ? "Finishes after" : "Blocked by"} ${labelForTarget(ref)}`
                            : `${dep.scope === "finish" ? "Must finish before" : "Blocks"} ${labelForTarget(dep.from_card_id)}`}
                        </span>
                      </div>
                    );
                  })
                ) : (
                  <p className="text-xs text-muted-foreground">
                    No linked cards or questions.
                  </p>
                )}
              </section>
              {card.open_question_count > 0 && (
                <p className="mt-4 flex items-center gap-2 text-xs text-muted-foreground">
                  <CircleHelp className="size-4" /> {card.open_question_count}{" "}
                  open question{card.open_question_count === 1 ? "" : "s"}{" "}
                  linked
                </p>
              )}
            </TabPanel>
            <div className="flex items-center justify-between gap-3 border-t bg-card px-5 py-3">
              <span className="text-xs text-muted-foreground">
                Version {card.version}
              </span>
              <span className="text-xs text-muted-foreground">{card.owner_actor ? `Owned by ${card.owner_actor}` : "Unclaimed"}</span>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
