import assert from "node:assert/strict";
import test from "node:test";
import { folderName, pathProblem, projectNameFor } from "../src/lib/new-project.ts";

test("folder name is the last path segment", () => {
  assert.equal(folderName("/Users/me/code/app"), "app");
  assert.equal(folderName("/Users/me/code/app/"), "app");
  assert.equal(folderName("C:\\code\\app"), "app");
  assert.equal(folderName(""), "");
});

test("path validation messages", () => {
  assert.match(pathProblem("  "), /Choose/);
  assert.match(pathProblem("code/app"), /absolute/);
  assert.equal(pathProblem("/Users/me/app"), null);
  assert.equal(pathProblem("~/app"), null);
});

test("typed name wins over the folder name", () => {
  assert.equal(projectNameFor("", "/x/app"), "app");
  assert.equal(projectNameFor(" Mine ", "/x/app"), "Mine");
});
