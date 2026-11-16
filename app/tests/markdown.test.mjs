import assert from "node:assert/strict";
import test from "node:test";
import { parseInline, parseMarkdown } from "../src/lib/markdown.ts";

test("headings, paragraphs and lists", () => {
  const blocks = parseMarkdown("# Title\n\nSome **bold** text\nsecond line\n\n- a\n- b\n\n1. x\n2. y");
  assert.deepEqual(blocks.map((b) => b.kind), ["heading", "paragraph", "list", "list"]);
  assert.equal(blocks[0].level, 1);
  assert.equal(blocks[2].items.length, 2);
  assert.equal(blocks[3].ordered, true);
  assert.equal(blocks[1].inline[1].kind, "strong");
});

test("fenced code keeps its text verbatim, including markdown and html", () => {
  const [block] = parseMarkdown("```sh\n# not a heading\n<b>x</b>\n```");
  assert.equal(block.kind, "code");
  assert.equal(block.text, "# not a heading\n<b>x</b>");
});

test("tables parse header and rows", () => {
  const [table] = parseMarkdown("| a | b |\n|---|---|\n| 1 | `2` |");
  assert.equal(table.kind, "table");
  assert.equal(table.header.length, 2);
  assert.equal(table.rows[0][1][0].kind, "code");
});

test("only http, mailto and anchor links become links", () => {
  assert.equal(parseInline("[x](https://a.b)")[0].kind, "link");
  assert.equal(parseInline("[x](javascript:alert(1))")[0].kind, "text");
});

test("raw html stays text", () => {
  const [p] = parseMarkdown("<script>alert(1)</script>");
  assert.equal(p.inline[0].kind, "text");
});

test("tryParseMarkdown returns blocks, and null instead of throwing", async () => {
  const { tryParseMarkdown } = await import("../src/lib/markdown.ts");
  assert.equal(tryParseMarkdown("# hi")[0].kind, "heading");
  assert.equal(tryParseMarkdown(null), null);
});

test("raw html stays text", () => {
  const [p] = parseMarkdown("<script>alert(1)</script>");
  assert.equal(p.kind, "paragraph");
  assert.equal(p.inline.every((n) => n.kind === "text"), true);
});
