// The options page: where the bridge is, and whether it answers.
//
// The check is a real request for a real title, not a ping. The bridge has
// one route and it acts, so there is nothing to ask that does not start a
// search -- which means the probe sends something nobody will search for and
// says so in the label. `doris-extension-check` is the kind of query that
// matches nothing, and a search that finds nothing costs one poll.

/** Where the bridge lives, unless the options page says otherwise. */
const DEFAULT_BRIDGE = "http://127.0.0.1:14141";

/** A query that matches no tracker, used for the check. */
const PROBE = "doris-extension-check";

const input = document.getElementById("bridge");
const status = document.getElementById("status");

function say(text, kind) {
  status.textContent = text;
  status.className = kind || "";
}

function bridge() {
  const value = input.value.trim();
  return value || DEFAULT_BRIDGE;
}

async function load() {
  const { bridge: saved } = await browser.storage.local.get(["bridge"]);
  input.value = typeof saved === "string" ? saved : "";
}

async function save() {
  const value = input.value.trim();
  if (value) {
    await browser.storage.local.set({ bridge: value });
    say("saved", "ok");
  } else {
    // Empty is `auto`, and writing an empty string would pin it to one.
    await browser.storage.local.remove(["bridge"]);
    say(`saved (default: ${DEFAULT_BRIDGE})`, "ok");
  }
}

/**
 * Ask the bridge whether it is there.
 *
 * A refused answer means it is running and doris does not recognise this
 * page -- which from the options page is a mistake worth showing, since the
 * options page is not one of the four sites the add-on has a button on.
 */
async function check() {
  const base = bridge();
  say("checking…");
  const url = `${base.replace(/\/+$/, "")}/search?q=${encodeURIComponent(PROBE)}`;
  try {
    const response = await fetch(url, {
      method: "GET",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: { Accept: "application/json" },
    });
    const body = await response.json().catch(() => ({}));
    if (response.ok) {
      say("doris is listening and accepted it", "ok");
    } else {
      say(
        `refused (${response.status}): ${body.error || "no reason given"}. ` +
          "The bridge answers an allow list; this page's origin is not on it.",
        "bad",
      );
    }
  } catch (e) {
    say("nothing is listening there", "bad");
  }
}

input.addEventListener("change", save);
input.addEventListener("blur", save);
document.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    save();
    check();
  }
});

const button = document.createElement("button");
button.textContent = "Save and check";
button.addEventListener("click", () => {
  save().then(check);
});
input.insertAdjacentElement("afterend", button);

load();