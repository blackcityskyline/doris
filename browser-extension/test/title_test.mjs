// The title extraction, run under node.
//
// It is the part with a bug in it: the button has to know the title, and
// every site spells that differently. A stub document is enough to ask the
// question -- "given this hostname and these elements, what would it search
// for?" -- and it runs with no dependencies and no browser.
//
//     node browser-extension/test/title_test.mjs

import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const { clean, titleFor, isTitlePage, actionFor, SITES } = require(join(here, "..", "title.js"));

/**
 * The smallest document that answers the question: `querySelector` over a
 * list of `{selector, text}` pairs, and a hostname.
 */
function doc(hostname, elements, ogTitle) {
  const bySelector = new Map(elements.map((e) => [e.selector, e]));
  return {
    location: { hostname },
    querySelector(selector) {
      if (selector === 'meta[property="og:title"]') {
        return ogTitle ? { getAttribute: () => ogTitle } : null;
      }
      const found = bySelector.get(selector);
      return found ? { textContent: found.text, getAttribute: () => found.text } : null;
    },
  };
}

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

// --- clean ---------------------------------------------------------------

it("drops a trailing year, which no release is called", () => {
  assert.equal(clean("Dune: Part Two (2024)"), "Dune: Part Two");
});

it("drops a year and a rating in one go", () => {
  assert.equal(clean("Dune (2024) 8.5"), "Dune");
});

it("keeps punctuation a tracker indexes", () => {
  assert.equal(clean("Marvel's Avengers: Infinity War"), "Marvel's Avengers: Infinity War");
});

it("keeps a year that is part of the name", () => {
  assert.equal(clean("Blade Runner 2049"), "Blade Runner 2049");
});

// The exact string a real IMDb page handed the add-on, which searched for
// all of it and found nothing:
//
//   East of Eden (TV Mini Series 2026) - IMDb
//
// Three separate things wrong with it: the site's own name, a bracketed
// qualifier that is IMDb's description of the entry rather than part of the
// name, and the year inside that qualifier.
it("takes a real IMDb title back to the name a tracker has", () => {
  assert.equal(clean("East of Eden (TV Mini Series 2026) - IMDb"), "East of Eden");
  assert.equal(clean("Marvels Daredevil (TV Series 2015) - IMDb"), "Marvels Daredevil");
  assert.equal(clean("Dune: Part Two (2024) - IMDb"), "Dune: Part Two");
});

it("takes the other sites' own names off too", () => {
  assert.equal(clean("Severance (2022) - Trakt"), "Severance");
  assert.equal(clean("Довод (2019) - Кинопоиск"), "Довод");
  assert.equal(clean("The Bear (2018) - Lampa"), "The Bear");
});

it("strips a rating only with the year it hangs on", () => {
  assert.equal(clean("Dune (2024) 8.5"), "Dune");
  // A number that is part of the name stays: there is no telling "Dune 8.5"
  // from a rating except the year next to it, and removing it on its own
  // turns "Ocean's 8" into "Ocean's".
  assert.equal(clean("Ocean's 8"), "Ocean's 8");
  assert.equal(clean("Fahrenheit 451"), "Fahrenheit 451");
  assert.equal(clean("1917 (2019)"), "1917");
});

it("keeps a release group's punctuation", () => {
  assert.equal(
    clean("Some.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-NTb"),
    "Some.Show.S01E01.1080p.WEB-DL.DDP5.1.H.264-NTb",
  );
});

it("collapses the non-breaking spaces a site uses", () => {
  assert.equal(clean("The Good Place"), "The Good Place");
});

it("strips the media type IMDb puts in the heading", () => {
  assert.equal(clean("Movie Interstellar"), "Interstellar");
});

it("has nothing to return for nothing", () => {
  assert.equal(clean(""), "");
  assert.equal(clean(null), "");
  assert.equal(clean(undefined), "");
});

// --- per site ------------------------------------------------------------

it("reads IMDb's hero heading", () => {
  const page = doc("www.imdb.com", [
    { selector: "[data-testid='hero-heading-pageTitle']", text: "Dune: Part Two" },
  ]);
  assert.equal(titleFor(page), "Dune: Part Two");
});

it("falls through IMDb's selectors to og:title", () => {
  const page = doc(
    "imdb.com",
    [{ selector: "h1.titleHeader", text: "Arrival" }],
    "Arrival (2016)",
  );
  assert.equal(titleFor(page), "Arrival");
});

it("reads Trakt's heading and drops its year", () => {
  const page = doc("trakt.tv", [
    { selector: "h1[data-testid='show-title']", text: "Severance (2022)" },
  ]);
  assert.equal(titleFor(page), "Severance");
});

it("reads Kinopoisk's name property", () => {
  const page = doc("kinopoisk.ru", [
    { selector: "h1[itemprop='name']", text: "Изгой" },
  ]);
  assert.equal(titleFor(page), "Изгой");
});

it("keeps a Cyrillic title intact", () => {
  const page = doc("www.lampa.mx", [
    { selector: ".details h1", text: "Изгой  (2021)" },
  ]);
  assert.equal(titleFor(page), "Изгой");
});

it("uses og:title on a site it has no table for", () => {
  const page = doc("example.org", [], "Something Else (2020)");
  assert.equal(titleFor(page), "Something Else");
});

it("returns nothing when there is no title at all", () => {
  const page = doc("example.org", [], null);
  assert.equal(titleFor(page), "");
});

it("does not let a selector from another site answer", () => {
  // Kinopoisk's selector on an IMDb page: the table is chosen by hostname,
  // so an IMDb page with a kinopoisk-shaped element still reads IMDb's.
  const page = doc(
    "www.imdb.com",
    [{ selector: "h1[itemprop='name']", text: "Wrong" }],
    "Right (2020)",
  );
  assert.equal(titleFor(page), "Right");
});

// --- is this page about one film at all ----------------------------------

/**
 * The same stub, plus a path -- the address is what decides, not the title.
 */
function at(hostname, pathname) {
  return {
    location: { hostname, pathname },
    querySelector: () => null,
  };
}

// IMDb's front page is called "Most Anticipated This Month", Trakt's is a
// dashboard. Clicking the button there used to send that heading to the
// bridge, which answered 200, so the button said `sent` and the search found
// nothing: a failure that looked like the bridge's and was the button's.
it("knows an IMDb title page from the front page", () => {
  assert.equal(isTitlePage(at("www.imdb.com", "/title/tt1160419/")), true);
  assert.equal(isTitlePage(at("www.imdb.com", "/")), false);
  assert.equal(isTitlePage(at("www.imdb.com", "/chart/")), false);
  assert.equal(isTitlePage(at("www.imdb.com", "/search/title/?title=Dune")), false);
});

it("knows Trakt's show and movie pages from its dashboard", () => {
  assert.equal(isTitlePage(at("trakt.tv", "/shows/the-office")), true);
  assert.equal(isTitlePage(at("trakt.tv", "/movies/dune-part-two")), true);
  assert.equal(isTitlePage(at("trakt.tv", "/dashboard")), false);
});

it("knows the other two", () => {
  assert.equal(isTitlePage(at("kinopoisk.ru", "/film/12345")), true);
  assert.equal(isTitlePage(at("kinopoisk.ru", "/")), false);
  assert.equal(isTitlePage(at("www.lampa.mx", "/film/abc")), true);
  assert.equal(isTitlePage(at("www.lampa.mx", "/catalog")), false);
});

it("lets an unknown site through rather than guessing", () => {
  // Refusing a page we have no rule for would be deciding that somebody's
  // own site has no films on it.
  assert.equal(isTitlePage(at("example.org", "/whatever")), true);
  assert.equal(isTitlePage(at("example.org", "/")), true);
});

it("gives every site a rule or says why it has none", () => {
  for (const site of SITES) {
    assert.ok(
      Array.isArray(site.paths) && site.paths.length > 0,
      `${site.hosts[0]} has no path rule, so its section pages would be searched`,
    );
  }
});

// --- what the button does -------------------------------------------------

/**
 * A page with a title, at an address. Both matter: the address says whether
 * the page is about one film, the title says what to send.
 */
function page(hostname, pathname, heading) {
  const d = doc(hostname, [{ selector: "h1", text: heading }], heading);
  d.location.pathname = pathname;
  return d;
}

// The decision lives in title.js and not in content.js precisely so this can
// be asked without a DOM. A rule that decides whether to search is the rule
// worth checking, and content.js is not checked at all.
it("sends on a title page", () => {
  assert.equal(actionFor(page("www.imdb.com", "/title/tt1160419/", "Dune: Part Two")), "send");
  assert.equal(actionFor(page("trakt.tv", "/shows/x", "Severance (2022)")), "send");
});

it("refuses a section page, and says which kind of refusal it is", () => {
  // The two are different messages because they are different problems: one
  // page is not about a film, the other has no name to send.
  assert.equal(actionFor(page("www.imdb.com", "/", "Most Anticipated")), "not-a-title");
  assert.equal(actionFor(page("trakt.tv", "/dashboard", "Trending")), "not-a-title");
  assert.equal(actionFor(page("www.imdb.com", "/title/tt1/", "")), "no-title");
});

it("lets an unknown site through", () => {
  assert.equal(actionFor(page("example.org", "/anything", "Something (2020)")), "send");
});

// --- the table itself ----------------------------------------------------

it("lists every site with at least one selector", () => {
  for (const site of SITES) {
    assert.ok(site.hosts.length > 0, `${site.hosts} has no host`);
    assert.ok(site.selectors.length > 0, `${site.hosts[0]} has no selector`);
  }
});

it("has one entry per host, so a hostname picks exactly one", () => {
  const all = SITES.flatMap((s) => s.hosts);
  assert.equal(new Set(all).size, all.length, `duplicate host: ${all}`);
});

console.log(`\n${passed} checks passed`);