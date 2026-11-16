import assert from "node:assert/strict";
import test from "node:test";
import { CARD_TABS, initialTab, nextTab, tabBadges } from "../src/lib/card-tabs.ts";

const card = (extra = {}) => ({ open_question_count: 0, blocked: false, blockers: [], review_gates: [], session: null, ...extra });

test("a plain card opens on overview with no badges", () => {
  assert.equal(initialTab(card()), "overview");
  assert.deepEqual(tabBadges(card()), {});
});

test("an open question opens the links tab and badges it with a count", () => {
  const c = card({ open_question_count: 2 });
  assert.equal(initialTab(c), "links");
  assert.equal(tabBadges(c).links.count, 2);
});

test("a waiting review opens the code tab, ahead of a question", () => {
  const c = card({ open_question_count: 1, review_gates: [{ approved: false }, { approved: true }] });
  assert.equal(initialTab(c), "code");
  assert.equal(tabBadges(c).code.count, 1);
});

test("approved reviews carry no badge", () => {
  assert.equal(tabBadges(card({ review_gates: [{ approved: true }] })).code, undefined);
});

test("a running agent shows a dot on activity; paused or held does not", () => {
  assert.equal(tabBadges(card({ session: { paused: false, held_by: null } })).activity.kind, "dot");
  assert.equal(tabBadges(card({ session: { paused: true } })).activity, undefined);
  assert.equal(tabBadges(card({ session: { paused: false, held_by: "human" } })).activity, undefined);
});

test("blockers badge the overview only while blocked", () => {
  assert.equal(tabBadges(card({ blocked: true, blockers: [{}, {}] })).overview.count, 2);
  assert.equal(tabBadges(card({ blocked: false, blockers: [{}] })).overview, undefined);
});

test("a remembered tab wins over attention", () => {
  assert.equal(initialTab(card({ open_question_count: 1 }), "history"), "history");
  assert.equal(initialTab(card(), "bogus"), "overview");
});

test("arrow keys wrap and Home/End jump", () => {
  assert.equal(nextTab("overview", "ArrowRight"), "activity");
  assert.equal(nextTab("overview", "ArrowLeft"), CARD_TABS.at(-1));
  assert.equal(nextTab("links", "ArrowRight"), "overview");
  assert.equal(nextTab("code", "Home"), "overview");
  assert.equal(nextTab("code", "End"), "links");
  assert.equal(nextTab("code", "a"), null);
});
