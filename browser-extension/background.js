// The network half, and the only place the bridge's address is used.
//
// In the background script rather than the content script because a content
// script fetches from the page's origin: it would send IMDb's `Origin`, be
// answered by CORS, and be at the mercy of whatever the page does to its
// globals. From here it is doris the request is from.

/** Where the bridge lives, unless the options page says otherwise. */
const DEFAULT_BRIDGE = "http://127.0.0.1:14141";

/** Where the queue lives between runs. */
const STORE = "pending";

async function bridge() {
  const { bridge: saved } = await browser.storage.local.get(["bridge"]);
  return (typeof saved === "string" && saved) || DEFAULT_BRIDGE;
}

/**
 * Ask whether doris is there. It does nothing when it answers -- it exists
 * so that finding out does not start a search.
 */
async function alive() {
  try {
    const response = await fetch(`${(await bridge()).replace(/\/+$/, "")}/ping`, {
      method: "GET",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: { Accept: "application/json" },
    });
    return response.ok;
  } catch (e) {
    return false;
  }
}

/**
 * Send a title to doris.
 *
 * `127.0.0.1` and not `localhost`: on a machine where one resolves to ::1
 * and the daemon listens on 127.0.0.1 only, the request goes to an address
 * nothing is listening on, and the browser reports a connection failure for
 * a daemon that is right there.
 *
 * A refused connection is not the end of it. The bridge is a server inside
 * the app, so with doris closed there is nobody to receive anything; the
 * title is queued and sent when doris comes back. `queued: true` says so to
 * the caller rather than letting it say `sent` for a search that has not
 * happened yet.
 */
async function search(title) {
  const base = (await bridge()).replace(/\/+$/, "");
  try {
    const response = await fetch(`${base}/search?q=${encodeURIComponent(title)}`, {
      method: "GET",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: { Accept: "application/json" },
    });
    const body = await response.json().catch(() => ({}));
    if (!response.ok) {
      return { ok: false, error: (body && body.error) || `HTTP ${response.status}` };
    }
    return { ok: true, query: body.query };
  } catch (e) {
    await queueNow(title);
    return { ok: false, queued: true, error: "waiting for doris" };
  }
}

/** Put a title in the queue and start the poll that will empty it. */
async function queueNow(title) {
  const { [STORE]: stored } = await browser.storage.local.get([STORE]);
  const queue = dorisQueue.add(dorisQueue.read(stored), title);
  await browser.storage.local.set({ [STORE]: queue });
  await badge();
  schedule();
  return queue;
}

/**
 * What a page asks while it waits: is doris up yet, and how much is left.
 *
 * The page asks *us*, every couple of seconds, while a title is queued. The
 * other direction -- us telling the page -- does not arrive: measured, a
 * title delivered with the button still saying `queued` beside a finished
 * search. A message sent from a page wakes this event page even when Firefox
 * has suspended it, so asking is also what makes the answer possible between
 * two alarms. The 30-second alarm stays for the case where every page is
 * closed, which is the case nothing else can cover.
 */
async function status() {
  const { [STORE]: stored } = await browser.storage.local.get([STORE]);
  const before = dorisQueue.read(stored).length;
  const answer = before ? await drain() : null;
  const { [STORE]: after } = await browser.storage.local.get([STORE]);
  const left = dorisQueue.read(after).length;
  return {
    delivered: answer && answer.ok ? answer.query : null,
    queued: left,
    // Something was waiting, now nothing is, and no send says it went: the
    // queue emptied where the page could not see it. Said, rather than left
    // as "still waiting", which would blame a queue that is empty.
    vanished: before > 0 && left === 0 && !(answer && answer.ok),
  };
}

/**
 * Send one waiting title, if doris is there.
 *
 * The ping comes first because it is the free question: without it the only
 * way to find out whether a send will land is to send, and a send that lands
 * starts a search -- so "retry" would be a search every two seconds.
 */
async function drain() {
  const { [STORE]: stored } = await browser.storage.local.get([STORE]);
  const queue = dorisQueue.read(stored);
  if (!queue.length) {
    schedule();
    return null;
  }
  if (!(await alive())) {
    schedule();
    return null;
  }
  const next = dorisQueue.take(queue);
  const answer = await search(next.title);
  if (answer.ok) {
    await browser.storage.local.set({ [STORE]: next.queue });
    // Written because the store is the channel that works: a message from
    // here to a content script does not arrive (measured -- a title
    // delivered with the button still saying `queued` beside a finished
    // search), so the page reads this rather than being told.
    await browser.storage.local.set({
      lastTitle: next.title,
      lastSentAt: Date.now(),
    });
    await badge();
    // More may be waiting: the next one goes now rather than on the next
    // alarm, because doris has just proved it is there.
    if (next.queue.length) await drain();
    else schedule();
    return answer;
  }
  await browser.storage.local.set({ [STORE]: next.queue.concat(next.title) });
  schedule();
  return answer;
}

/** The toolbar badge: how many titles are waiting for doris. */
async function badge() {
  const { [STORE]: stored } = await browser.storage.local.get([STORE]);
  const waiting = dorisQueue.read(stored).length;
  await browser.action.setBadgeText({ text: waiting ? String(waiting) : "" });
}

/**
 * The poll: an alarm, not a timer.
 *
 * A `setTimeout` here worked exactly once and then never again -- an MV3
 * background is an event page, Firefox suspends it after about thirty
 * seconds of quiet, and a suspended page runs no timers. So the title was
 * queued, doris came back thirty seconds later, and nothing happened until
 * something else woke the page. `alarms` is the one clock that keeps going
 * while suspended.
 *
 * No alarm at all when nothing is waiting: an installed add-on that wakes
 * every thirty seconds for an app that is not running is an add-on nobody
 * leaves installed.
 */
const ALARM = "doris-queue";

/**
 * Two clocks, because one of them cannot go fast.
 *
 * The alarm is the floor of what a suspended event page will honour -- 30
 * seconds is the smallest period Firefox accepts, and asked for anything
 * less it clamps. The timeout is what makes it quick while the page is
 * awake: asked in a page's `setInterval`, this wakes it, and that chain runs
 * every 15. Between the two, a queued title goes out in about fifteen
 * seconds with the page open and thirty with every tab closed, and the
 * second case is the one the alarm exists for.
 */
let ticker = null;

function schedule() {
  if (ticker) {
    clearTimeout(ticker);
    ticker = null;
  }
  browser.storage.local.get([STORE]).then(({ [STORE]: stored }) => {
    if (dorisQueue.read(stored).length) {
      browser.alarms.create(ALARM, { periodInMinutes: dorisQueue.POLL_MINUTES });
      ticker = setTimeout(beat, dorisQueue.AWAKE_MS);
    } else {
      browser.alarms.clear(ALARM);
    }
  });
}

/** The fast half: re-arm, and look once while doing it. */
async function beat() {
  ticker = null;
  await drain();
  schedule();
}

browser.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === ALARM) beat();
});

browser.runtime.onMessage.addListener((message) => {
  if (!message) return undefined;
  if (message.type === "search") return search(message.title);
  if (message.type === "status") return status();
  return undefined;
});

// A title left over from a browser that was closed with doris closed is
// still a title the user pressed.
browser.runtime.onStartup.addListener(() => {
  schedule();
});
browser.runtime.onInstalled.addListener(() => {
  schedule();
});

/**
 * The toolbar button: search whatever the tab is.
 *
 * It is the way in from a page the add-on has no button on, which is any
 * site outside the four in the table. `tab.title` and not the page's `h1`:
 * a background page has no page to read, and the tab title is the title the
 * user is looking at anyway.
 */
/**
 * The toolbar button, and `Ctrl+Shift+D`: put the page into selection mode.
 *
 * Selecting beats reading. A page's own title is right when the page is
 * about one film and useless when it is not -- and even when it is right it
 * can be "East of Eden (TV Mini Series 2026) - IMDb", which no tracker has.
 * What the user circles is what they meant.
 *
 * The tab title stays as the fallback for a page with no add-on in it, which
 * is every site outside the four in the table: there is no content script to
 * tell, and sending the tab's title is better than sending nothing.
 */
async function pickOn(tab) {
  try {
    await browser.tabs.sendMessage(tab.id, { type: "pick" });
    return true;
  } catch (e) {
    return false; // no content script on this page
  }
}

browser.commands.onCommand.addListener(async (command) => {
  if (command !== "pick-title") return;
  const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
  if (tab && !(await pickOn(tab))) await searchFromTab(tab);
});

browser.action.onClicked.addListener(async (tab) => {
  if (await pickOn(tab)) {
    await browser.action.setTitle({ title: "select a title · Esc to cancel", tabId: tab.id });
    return;
  }
  await searchFromTab(tab);
});

async function searchFromTab(tab) {
  const title = (tab.title || "").trim();
  if (!title) return;
  const answer = await search(title);
  const text = answer.ok
    ? `sent to doris: ${title}`
    : answer.queued
      ? `queued for doris: ${title}`
      : `doris: ${answer.error}`;
  await browser.action.setTitle({ title: text, tabId: tab.id });
  if (answer.queued) await badge();
}