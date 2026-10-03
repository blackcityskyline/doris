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
const { clean, titleFor, SITES } = require(join(here, "..", "title.js"));

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