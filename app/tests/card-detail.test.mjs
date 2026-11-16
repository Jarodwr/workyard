import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// Render-free structure check: the component is not renderable under node:test,
// so read its source. Progress is shown once, in the pinned header.
const source = readFileSync(new URL("../src/components/workspace/card-detail.tsx", import.meta.url), "utf8");

test("the task modal renders exactly one progress bar", () => {
  assert.equal(source.match(/<Progress[\s/>]/g)?.length, 1);
});

test("the task modal shows the progress percentage once", () => {
  assert.equal(source.match(/\{card\.progress\}%/g)?.length, 1);
});

test("the overview section is headed Status, not Agent progress", () => {
  assert.ok(source.includes(">Status</h3>"));
  assert.ok(!source.includes("Agent progress"));
  assert.ok(source.includes("No status update yet."));
});
