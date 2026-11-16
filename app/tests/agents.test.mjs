import assert from "node:assert/strict";
import test from "node:test";
import { ESTIMATE_CAP, estimateProgress, permissionsFor, progressDisplay, runRuleFor } from "../src/lib/agents.ts";

const running = (progress, extra = {}) => ({
  progress,
  open_question_count: 0,
  session: { runner: "claude", paused: false, held_by: null, ...extra },
});

test("estimate starts at zero and rises with tool uses and time", () => {
  assert.equal(estimateProgress(0, 0), 0);
  assert.ok(estimateProgress(10, 0) > estimateProgress(5, 0));
  assert.ok(estimateProgress(0, 600) > estimateProgress(0, 60));
});

test("estimate never passes the cap", () => {
  assert.equal(estimateProgress(100000, 1e9), ESTIMATE_CAP);
  assert.ok(estimateProgress(500, 36000) <= ESTIMATE_CAP);
});

test("reported progress wins over an estimate", () => {
  assert.deepEqual(progressDisplay(running(40, { tool_uses: 50 })), { value: 40, estimated: false });
});

test("a running card at 0 shows an estimate", () => {
  const shown = progressDisplay(running(0, { tool_uses: 20, elapsed_secs: 0 }));
  assert.equal(shown.estimated, true);
  assert.ok(shown.value > 0 && shown.value <= ESTIMATE_CAP);
});

test("a paused card at 0 stays at 0", () => {
  const card = running(0, { paused: true, tool_uses: 20 });
  assert.deepEqual(progressDisplay(card), { value: 0, estimated: false });
});

test("allow-for-this-run grants a command's first word, or the whole tool", () => {
  assert.equal(runRuleFor({ tool_name: "Bash", input: { command: "  make test --all" } }), "Bash(make *)");
  assert.equal(runRuleFor({ tool_name: "Bash", input: {} }), "Bash");
  assert.equal(runRuleFor({ tool_name: "WebFetch", input: { url: "https://x" } }), "WebFetch");
});

test("only pending requests on that card are shown", () => {
  const requests = [
    { id: "1", card_key: "C-1", status: "pending" },
    { id: "2", card_key: "C-1", status: "denied" },
    { id: "3", card_key: "C-2", status: "pending" },
  ];
  assert.deepEqual(permissionsFor({ key: "C-1" }, requests).map((r) => r.id), ["1"]);
});
