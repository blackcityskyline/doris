// Titles pressed while doris was not running.
//
// The bridge is a server *inside* the app: with doris closed nothing is
// listening, so a title sent then has nowhere to go and is lost. This is
// the store that keeps it, and the add-on drains it as soon as a `/ping`
// comes back saying doris is there.
//
// Pure functions over a plain array, so the whole thing -- what goes in, what
// comes out, when two clicks are the same click -- is checkable without a
// browser, which is the only kind of check that will run on every machine.
//
// A plain script, not a module. A content script is classic JavaScript, and
// one `export` anywhere in the file is a syntax error that takes the whole
// script list down with it -- so the button did not appear on any page, and
// nothing but a real browser could have said so.

/** How many titles are kept. */
const CAP = 20;

/**
 * Add a title, newest last.
 *
 * A title already in the list moves to the end rather than being added twice:
 * pressing the button twice on the same page is one search that was pressed
 * twice, and searching for it twice costs the user two result panes.
 */
function add(queue, title) {
  const clean = String(title || "").trim();
  if (!clean) return queue;
  const without = queue.filter((t) => t !== clean);
  const next = without.concat(clean);
  return next.length > CAP ? next.slice(next.length - CAP) : next;
}

/** Whether a title is already waiting, so the button can say so. */
function has(queue, title) {
  const clean = String(title || "").trim();
  return Boolean(clean) && queue.indexOf(clean) !== -1;
}

/**
 * The next title to send, and the queue without it.
 *
 * Oldest first: the user pressed them in that order, and a search that comes
 * back in a different order than the presses is a list that means nothing.
 */
function take(queue) {
  if (!queue.length) return { title: null, queue };
  return { title: queue[0], queue: queue.slice(1) };
}

/** Read the queue out of storage, tolerating anything that is not one. */
function read(value) {
  return Array.isArray(value) ? value.filter((t) => typeof t === "string" && t) : [];
}

/**
 * How often to look for doris, in minutes -- Firefox's floor.
 *
 * Not seconds, and not a `setTimeout`: an MV3 background is an *event page*,
 * which Firefox suspends after about half a minute of doing nothing, and a
 * suspended page does not run its timers. Measured: with `setTimeout`, a
 * title queued and then delivered fifty seconds later never was -- the
 * timer died before doris came back. `alarms` is the one timer a suspended
 * event page still honours.
 */
const POLL_MINUTES = 0.5;

/**
 * The fast clock, for while the event page is awake: 15 seconds, against
 * the alarm's 30-second floor. Exported because it is a promise about how
 * long somebody waits, and a promise like that wants a test.
 */
const AWAKE_MS = 15000;

if (typeof module !== "undefined" && module.exports) {
  module.exports = { CAP, POLL_MINUTES, AWAKE_MS, add, has, take, read };
} else {
  globalThis.dorisQueue = { CAP, POLL_MINUTES, AWAKE_MS, add, has, take, read };
}