import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight, MessageSquare, Send } from "lucide-react";
import { client, type Question, type QuestionMessage } from "@/api";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** Small marker for a question with agent replies a human has not looked at yet. */
export function UnreadReplyBadge({ question, className }: { question: Question; className?: string }) {
  const unread = question.unread_messages ?? 0;
  if (question.status !== "open" || unread < 1) return null;
  return <span
    className={cn("inline-flex items-center gap-1 rounded-full border border-primary/30 bg-primary/10 px-1.5 py-px text-[9px] font-semibold leading-3 text-primary", className)}
    title={`${unread} new ${unread === 1 ? "reply" : "replies"} from the agent`}
    aria-label={`${unread} new agent ${unread === 1 ? "reply" : "replies"}`}
  >
    <MessageSquare className="size-2.5" aria-hidden />{unread} new
  </span>;
}

function authorLabel(message: QuestionMessage): string {
  if (message.author_is_human) return message.author === "ui" ? "You" : message.author;
  if (message.author.startsWith("agent:supervisor-")) return "Supervisor";
  return "Agent";
}

/**
 * The conversation about one open question: messages oldest first and a box to write. A human
 * message wakes the agent, which replies here; the question stays open until a human answers it.
 * `collapsible` shows it behind a toggle (with the unread marker) and marks replies read when
 * it is open.
 */
export function QuestionThread({ question, collapsible = false, onError }: { question: Question; collapsible?: boolean; onError?: (message: string) => void }) {
  const [open, setOpen] = useState(!collapsible);
  const [messages, setMessages] = useState<QuestionMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const end = useRef<HTMLLIElement>(null);
  const isOpen = question.status === "open";
  const count = question.message_count ?? 0;
  const unread = question.unread_messages ?? 0;

  const load = useCallback(async () => {
    try {
      setMessages(await client.questionThread(question.key));
    } catch (error) {
      onError?.(String(error));
    }
  }, [question.key, onError]);

  // Refetch when the list says the thread changed (every commit refreshes the question list).
  useEffect(() => {
    if (open) void load();
  }, [open, load, count]);

  // Looking at the thread clears its unread marker.
  useEffect(() => {
    if (open && unread > 0) void client.markQuestionRead(question.key).catch(() => undefined);
  }, [open, unread, question.key]);

  useEffect(() => {
    end.current?.scrollIntoView?.({ block: "nearest" });
  }, [messages.length]);

  async function send() {
    const body = draft.trim();
    if (!body || sending) return;
    setSending(true);
    try {
      await client.discussQuestion(question.key, body);
      setDraft("");
      await load();
    } catch (error) {
      onError?.(String(error));
    } finally {
      setSending(false);
    }
  }

  const header = collapsible
    ? <button type="button" aria-expanded={open} onClick={() => setOpen((value) => !value)} className="flex w-full items-center gap-1.5 text-xs font-medium hover:text-primary">
        {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        <MessageSquare className="size-3.5" />Discuss{count > 0 ? ` (${count})` : ""}
        <UnreadReplyBadge question={question} />
      </button>
    : <div className="flex items-center gap-1.5 text-xs font-medium"><MessageSquare className="size-3.5" />Discuss{count > 0 ? ` (${count})` : ""}</div>;

  return <section aria-label={`Discussion of ${question.key}`} className="space-y-2 rounded-lg border bg-background/60 px-3 py-2.5">
    {header}
    {open && <>
      {messages.length === 0
        ? <p className="text-xs text-muted-foreground">No messages yet. Ask the agent about its options; it replies here and the question stays open until you answer it.</p>
        : <ul className="max-h-56 space-y-2 overflow-auto pr-1" aria-live="polite">
            {messages.map((message) => <li key={message.id} className={cn("rounded-md border px-2.5 py-1.5 text-xs", message.author_is_human ? "ml-6 border-primary/25 bg-primary/5" : "mr-6 bg-muted/40")}>
              <div className="mb-0.5 flex items-center justify-between gap-2 text-[10px] text-muted-foreground"><span className="font-semibold">{authorLabel(message)}</span><time dateTime={message.created_at}>{new Date(message.created_at).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</time></div>
              <p className="whitespace-pre-wrap break-words leading-relaxed">{message.body}</p>
            </li>)}
            <li ref={end} aria-hidden className="h-0" />
          </ul>}
      {isOpen
        ? <div className="flex items-end gap-2">
            <textarea
              aria-label={`Message about ${question.key}`}
              className="min-h-[2.25rem] w-full flex-1 resize-y rounded-md border bg-background p-2 text-sm"
              rows={2}
              maxLength={8000}
              placeholder="Ask the agent or push back…  (Cmd+Enter to send)"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); void send(); } }}
            />
            <Button size="sm" disabled={!draft.trim() || sending} onClick={() => void send()}><Send className="size-3.5" />Send</Button>
          </div>
        : <p className="text-xs text-muted-foreground">This question is resolved; the discussion is closed.</p>}
    </>}
  </section>;
}
