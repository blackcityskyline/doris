// What to search for, given the page you are on.
//
// A function, not a script, because it is the part with a bug in it: the
// button has to know the title, and every site spells that differently. The
// table is the whole of it, and the fallback is `og:title`, which is what a
// page says its own name is -- the one thing every site gets right.
//
// Loaded by both the background script and the content script, and runnable
// under node (see `test/title_test.mjs`), because "which selector" is a
// question with a right answer that can be checked without a browser.

/**
 * Where the title lives, per host. First selector that matches wins.
 *
 * `imdb.com` and the others are matched on the hostname, not the URL, so a
 * page's own query string cannot choose the rule.
 */
const SITES = [
  {
    // The hero heading is the title as the page presents it; `og:title` on
    // IMDb appends the year and the rating in some locales.
    hosts: ["imdb.com", "www.imdb.com"],
    selectors: [
      "[data-testid='hero-heading-pageTitle']",
      "h1 .title_line",
      "h1.titleHeader",
    ],
  },
  {
    hosts: ["trakt.tv", "www.trakt.tv"],
    selectors: ["h1[data-testid='show-title']", "h1.show-title", "h1"],
  },
  {
    hosts: ["kinopoisk.ru", "www.kinopoisk.ru"],
    selectors: ["h1[itemprop='name']", "h1.[itemprop='name']", "h1"],
  },
  {
    hosts: ["lampa.mx", "www.lampa.mx"],
    selectors: [".details h1", "h1"],
  },
];

/**
 * Clean a title down to what a search should be given.
 *
 * The site names, not the search terms: a tracker indexes the release name,
 * so stripping punctuation would make the query miss. What goes is the
 * decoration a page adds around the name -- a trailing year on a Trakt
 * heading, a rating in brackets -- because no release is called that.
 */
function clean(raw) {
  if (!raw) return "";
  let title = String(raw)
    // "Dune: Part Two (2024)" -> "Dune: Part Two"
    .replace(/\s*\(\s*(?:19|20)\d{2}\s*\)\s*$/, "")
    // "Dune (2024) 8.5" -> "Dune"
    .replace(/\s*\(\s*(?:19|20)\d{2}\s*\)\s+[\d.]+\s*$/, "")
    // IMDb puts the type in the heading on some layouts: "Movie Dune (2024)".
    .replace(/^(?:Movie|Series|Episode|Video Game)\s+/i, "")
    // Collapse whitespace, including the non-breaking spaces a site uses to
    // stop a title from wrapping.
    .replace(/[\s ]+/g, " ")
    .trim();
  return title;
}

/**
 * The title for a document, or `""` when there is nothing to search for.
 *
 * `doc` is anything with `querySelector` and a `location`, so this runs
 * under node against a stub and in a content script against the real page.
 */
function titleFor(doc) {
  const meta = doc.querySelector('meta[property="og:title"]');
  const fallback = meta ? meta.getAttribute("content") : "";

  const host = (doc.location && doc.location.hostname) || "";
  const site = SITES.find((s) => s.hosts.includes(host));

  if (site) {
    for (const selector of site.selectors) {
      const element = doc.querySelector(selector);
      const text = element && (element.textContent || "");
      if (clean(text)) return clean(text);
    }
  }
  return clean(fallback);
}

/** A human name for the site, for the button's tooltip and the log. */
function siteName(doc) {
  const host = (doc.location && doc.location.hostname) || "this page";
  const site = SITES.find((s) => s.hosts.includes(host));
  return site ? site.hosts[0] : host;
}

// Both callers are browsers with a global; node wants a module.
if (typeof module !== "undefined" && module.exports) {
  module.exports = { SITES, clean, titleFor, siteName };
} else {
  globalThis.doris = { SITES, clean, titleFor, siteName };
}