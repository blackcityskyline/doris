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
    paths: ["/title/"],
    selectors: [
      "[data-testid='hero-heading-pageTitle']",
      "h1 .title_line",
      "h1.titleHeader",
    ],
  },
  {
    hosts: ["trakt.tv", "www.trakt.tv"],
    selectors: ["h1[data-testid='show-title']", "h1.show-title", "h1"],
    // A page whose address is one of these is about one film or show. Every
    // other page on the site is a section -- "Most Anticipated This Month",
    // "Coming Soon" -- and its title is not a title anybody can search for.
    paths: ["/shows/", "/movies/"],
  },
  {
    hosts: ["kinopoisk.ru", "www.kinopoisk.ru"],
    paths: ["/film/", "/series/", "/tv/", "/anime/"],
    selectors: ["h1[itemprop='name']", "h1.[itemprop='name']", "h1"],
  },
  {
    hosts: ["lampa.mx", "www.lampa.mx"],
    paths: ["/film", "/series", "/tv-series", "/anime"],
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
    // The site's own name at the end of its title, which no tracker has.
    // Measured on a real page: og:title on IMDb came back as
    // "East of Eden (TV Mini Series 2026) - IMDb", and searching for that
    // finds nothing at all.
    .replace(/\s+-\s+(?:IMDb|Trakt|Kinopoisk|Кинопоиск|Lampa|Лампа)\s*$/i, "")
    // A rating that IMDb put after the year: "Dune (2024) 8.5". It has to
    // come off together with the year it hangs on, because a rating on its
    // own cannot be told from a title that ends in a number -- "Ocean's 8"
    // is not a film called "Ocean's".
    .replace(/\s*\(\s*(?:19|20)\d{2}\s*\)\s+[\d.]+\s*$/, "")
    // A trailing bracketed qualifier carrying a year, whatever else is in
    // it: "(2024)" and "(TV Mini Series 2026)" both go, because both are
    // the site's description of the entry rather than part of the name. A
    // bracket with no year stays -- "(Director's Cut)" is in the title.
    .replace(/\s*\([^)]*\b(?:19|20)\d{2}[^)]*\)\s*$/, "")
    // IMDb puts the type in the heading on some layouts: "Movie Dune".
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

/**
 * Whether this page is about one thing that can be searched for.
 *
 * The button appears on every page of a site, and most of them are not about
 * one film: IMDb's front page is "Most Anticipated This Month", Trakt's is a
 * dashboard. Searching for those finds nothing, and the button says `sent`,
 * so the failure looks like the bridge's.
 *
 * The address is the test, not the title. A site with no `paths` here counts
 * as a title page: an unknown site is somebody's own page, and refusing it
 * would be guessing.
 */
function isTitlePage(doc) {
  const host = (doc.location && doc.location.hostname) || "";
  const site = SITES.find((s) => s.hosts.includes(host));
  if (!site || !site.paths) return true;
  const path = (doc.location && doc.location.pathname) || "";
  return site.paths.some((prefix) => path.startsWith(prefix));
}

/**
 * What the button should do on this page: `"send"`, `"not-a-title"` or
 * `"no-title"`.
 *
 * The decision is here rather than in `content.js` because it is a decision
 * about the *page* -- its address and its title -- and about nothing that
 * happens afterwards. Putting it here makes it testable without a DOM, which
 * is the only way it gets tested at all: `content.js` needs a browser, and a
 * rule that decides whether to search is exactly the rule worth checking.
 */
function actionFor(doc) {
  if (!isTitlePage(doc)) return "not-a-title";
  return titleFor(doc) ? "send" : "no-title";
}

/** A human name for the site, for the button's tooltip and the log. */
function siteName(doc) {
  const host = (doc.location && doc.location.hostname) || "this page";
  const site = SITES.find((s) => s.hosts.includes(host));
  return site ? site.hosts[0] : host;
}

// Both callers are browsers with a global; node wants a module.
if (typeof module !== "undefined" && module.exports) {
  module.exports = { SITES, clean, titleFor, isTitlePage, actionFor, siteName };
} else {
  globalThis.doris = { SITES, clean, titleFor, isTitlePage, actionFor, siteName };
}