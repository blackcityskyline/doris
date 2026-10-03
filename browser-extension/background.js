// The network half, and the only place the bridge's address is used.
//
// In the background script rather than the content script because a content
// script fetches from the page's origin: it would send IMDb's `Origin`, be
// answered by CORS, and be at the mercy of whatever the page does to its
// globals. From here it is doris the request is from.

/** Where the bridge lives, unless the options page says otherwise. */
const DEFAULT_BRIDGE = "http://127.0.0.1:14141";

/**
 * Send a title to doris.
 *
 * `127.0.0.1` and not `localhost`: on a machine where one resolves to ::1
 * and the daemon listens on 127.0.0.1 only, the request goes to an address
 * nothing is listening on, and the browser reports a connection failure for
 * a daemon that is right there.
 */
async function search(title) {
  const { bridge } = await browser.storage.local.get(["bridge"]);
  const base = (typeof bridge === "string" && bridge) || DEFAULT_BRIDGE;
  const url = `${base.replace(/\/+$/, "")}/search?q=${encodeURIComponent(title)}`;

  try {
    const response = await fetch(url, {
      method: "GET",
      // No credentials and no referrer: the request carries the title and
      // nothing about the page it came from.
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
    // A refused connection is the common case and it means one thing: doris
    // is not running. Saying so beats reporting a network error to someone
    // who cannot act on it.
    return { ok: false, error: "doris is not listening" };
  }
}

browser.runtime.onMessage.addListener((message) => {
  if (!message || message.type !== "search") return undefined;
  return search(message.title);
});

/**
 * The toolbar button: search whatever the tab is.
 *
 * It is the way in from a page the add-on has no button on, which is any
 * site outside the four in the table. `tab.title` and not the page's `h1`:
 * a background page has no page to read, and the tab title is the title the
 * user is looking at anyway.
 */
browser.action.onClicked.addListener(async (tab) => {
  const title = (tab.title || "").trim();
  if (!title) return;
  const answer = await search(title);
  await browser.action.setTitle({
    title: answer.ok ? `sent to doris: ${title}` : `doris: ${answer.error}`,
    tabId: tab.id,
  });
  if (browser.action.setBadgeText) {
    await browser.action.setBadgeText({
      text: answer.ok ? "✓" : "!",
      tabId: tab.id,
    });
    await browser.action.setBadgeBackgroundColor({
      color: answer.ok ? "#2e7d32" : "#b3261e",
      tabId: tab.id,
    });
  }
});