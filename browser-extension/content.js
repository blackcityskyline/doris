// The button, on the page.
//
// It asks the background script to send the title and then reports what came
// back *on the page*, next to the title the user is looking at. The
// alternative -- a silent button -- cannot be told apart from a broken one,
// and the search happens in another program on another screen, so "sent" is
// the true thing to report and anything about matches would be a lie.

/** The button's own class, so a second injection is a no-op. */
const MARK = "doris-search-button";

/**
 * Put the button after the title, so it reads as part of the heading.
 *
 * `insertBefore` on the title's sibling rather than `appendChild` on the
 * title: the title is usually inside a wrapper the page styled, and a button
 * hanging off the end of an `<h1>` lands on the wrong line.
 */
function place(titleElement) {
  const existing = document.querySelector(`.${MARK}`);
  if (existing) return existing;
  if (!titleElement || !titleElement.parentNode) return null;

  const button = document.createElement("button");
  button.className = MARK;
  button.type = "button";
  button.textContent = "doris";
  // The title the button was built for, on the button itself: it is what a
  // devtools inspector shows, and what a test reads to find out whether the
  // button agrees with the page.
  button.dataset.dorisTitle = doris.titleFor(document);
  button.title = `Search "${button.dataset.dorisTitle}" in doris`;
  // The page has its own click handlers on the heading, and some of them
  // navigate. This is not their click.
  button.addEventListener("click", (event) => {
    event.preventDefault();
    event.stopPropagation();
    send();
  });

  titleElement.parentNode.insertBefore(button, titleElement.nextSibling);
  return button;
}

/** Say what happened, in the button itself. */
function said(text, kind) {
  const button = document.querySelector(`.${MARK}`);
  if (!button) return;
  button.textContent = text;
  button.dataset.dorisState = kind;
}

/** Send the title this page is about. */
async function send() {
  // The decision is `doris.actionFor`, not a rule written twice here: it is
  // about the page, not about the click, and a second copy of it is a second
  // copy to get wrong.
  const action = doris.actionFor(document);
  if (action !== "send") {
    said(action === "not-a-title" ? "not a title" : "no title", "error");
    return;
  }
  const title = doris.titleFor(document);
  said("sending…", "busy");
  const answer = await browser.runtime.sendMessage({ type: "search", title });
  if (answer && answer.ok) {
    said("sent", "ok");
  } else if (answer && answer.queued) {
    // Not an error. The bridge is a server inside the app, so a title
    // pressed with doris closed has nowhere to go *yet*; it is kept and sent
    // when doris comes back, which is what the user meant by pressing it.
    // Saying `waiting for doris` and stopping there would be true and would
    // read as a failure.
    said("queued", "queued");
  } else {
    said((answer && answer.error) || "refused", "error");
  }
}

/**
 * Put the button on the page, if there is a title to put it next to.
 *
 * Re-run on a single-page app's navigation: IMDb and Trakt both replace the
 * heading without loading the page, so a button inserted once points at the
 * previous title.
 */
function install() {
  const action = doris.actionFor(document);
  // Refused on sight, so the button says so before it is pressed rather
  // than after.
  const refuse = {
    "not-a-title": ["not a title", "This page is not about one film or show"],
    "no-title": ["no title", "No title on this page"],
  }[action];
  if (refuse) {
    const button = place(document.querySelector("h1"));
    if (button) {
      button.textContent = refuse[0];
      button.dataset.dorisState = "error";
      button.title = refuse[1];
    }
    return;
  }
  const site = doris.SITES.find((s) => s.hosts.includes(document.location.hostname));
  const selectors = site ? site.selectors : ["h1"];
  const anchor = selectors.map((s) => document.querySelector(s)).find(Boolean);
  place(anchor);
}

install();

let lastUrl = location.href;
new MutationObserver(() => {
  if (location.href === lastUrl) return;
  lastUrl = location.href;
  // The heading was replaced, so the button that pointed at the old one went
  // with it. Re-adding rather than moving: after a navigation the two belong
  // to different elements.
  document.querySelectorAll(`.${MARK}`).forEach((b) => b.remove());
  install();
}).observe(document.body, { childList: true, subtree: true });