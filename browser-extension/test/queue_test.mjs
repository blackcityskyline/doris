// The queue that holds a title pressed while doris was closed.
//
// The bridge is a server inside the app. With doris closed there is nobody
// listening, so a title pressed then is not merely undelivered -- it is
// nowhere, unless something keeps it. This is that something, and it is
// where "I pressed it and doris was closed" stops being a lost click.
//
//     node browser-extension/test/queue_test.mjs

import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const queue = require(join(here, "..", "queue.js"));

let passed = 0;
function it(name, body) {
  try {
    body();
    passed += 1;
    console.log(`  ok  ${name}`);
  } catch (e) {
    console.error(`FAIL  ${name}\n      ${e.message}`);
    process.exitCode = 1;
  }
}

it("keeps a title", () => {
  assert.deepEqual(queue.add([], "Dune: Part Two"), ["Dune: Part Two"]);
});

it("keeps it in the order it was pressed", () => {
  // A queue that reorders is a queue where the second film the user looked
  // for turns up first, and the results pane belongs to the wrong one.
  let q = [];
  q = queue.add(q, "first");
  q = queue.add(q, "second");
  q = queue.add(q, "third");
  assert.deepEqual(q, ["first", "second", "third"]);
  const taken = queue.take(q);
  assert.equal(taken.title, "first");
  assert.deepEqual(taken.queue, ["second", "third"]);
});

it("pressing the same title twice is one search", () => {
  // The button can be pressed twice, and a page can be reloaded with the
  // button still there. Two searches for one film is two result panes and
  // one wasted fan-out.
  let q = queue.add([], "Dune");
  q = queue.add(q, "Other");
  q = queue.add(q, "Dune");
  assert.deepEqual(q, ["Other", "Dune"], "and it moves to the end, newest last");
});

it("says a title is already waiting", () => {
  const q = queue.add([], "Dune");
  assert.equal(queue.has(q, "Dune"), true);
  assert.equal(queue.has(q, "  Dune  "), true, "the same title, padded");
  assert.equal(queue.has(q, "Other"), false);
  assert.equal(queue.has(q, ""), false, "an empty title is not queued");
});

it("never grows without bound", () => {
  let q = [];
  for (let i = 0; i < queue.CAP + 25; i += 1) q = queue.add(q, `title ${i}`);
  assert.equal(q.length, queue.CAP, "the cap holds");
  assert.equal(q[q.length - 1], `title ${queue.CAP + 24}`, "the newest survives");
});

it("drops an empty title instead of storing it", () => {
  assert.deepEqual(queue.add([], ""), []);
  assert.deepEqual(queue.add([], "   "), []);
  assert.deepEqual(queue.add([], null), []);
});

it("takes nothing from an empty queue", () => {
  const taken = queue.take([]);
  assert.equal(taken.title, null);
  assert.deepEqual(taken.queue, []);
});

it("reads only a list of strings out of storage", () => {
  // Storage is whatever was in it last session. A number or an object must
  // not become a title that gets sent to a search.
  assert.deepEqual(queue.read(["Dune", 7, null, ""]), ["Dune"]);
  assert.deepEqual(queue.read("not a list"), []);
  assert.deepEqual(queue.read(undefined), []);
  assert.deepEqual(queue.read([{ title: "Dune" }]), []);
});

it("keeps a fast clock for when the page is awake", () => {
  // The alarm cannot go below Firefox's 30-second floor, so the fifteen
  // seconds the user asked for is a timeout chain -- which a page's messages
  // keep alive and which is gone when the browser suspends the event page.
  // The alarm stays for exactly that case.
  assert.equal(queue.AWAKE_MS, 15000, "asked for ten to fifteen seconds");
  assert.ok(
    queue.AWAKE_MS < queue.POLL_MINUTES * 60000,
    "and the fast clock is actually faster than the alarm",
  );
});

it("asks at least as often as an event page may be suspended", () => {
  // Firefox suspends an MV3 background after about thirty seconds of quiet,
  // and a suspended page runs no `setTimeout`. An alarm is the one clock
  // that keeps going, so the interval has to be at most that -- measured,
  // not guessed: with a plain two-second timer a queued title was never
  // delivered, because the timer died before doris came back.
  assert.ok(
    queue.POLL_MINUTES * 60 <= 30,
    `asks every ${queue.POLL_MINUTES * 60}s, which is longer than an event page lives`,
  );
  assert.ok(queue.POLL_MINUTES > 0, "and it actually asks");
});