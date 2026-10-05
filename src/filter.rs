//! The Results filter's syntax -- a grep-shaped mini-language rather than one opaque substring.
//! | token | matches | |-----------------|------------------------------------------------| |
//! `word` | substring of title+size+source+group | | `-word` | NOT that | | `src:id` | the
//! tracker id (`tracker:` is the alias) | | `group:name` | the category (`cat:` is the alias) |
//! | `title:word` | the title alone | | `size:>1gb` | `size_bytes`, units b/kb/mb/gb/tb | |
//! `seeds:>50` | `seeds_n` | Tokens are ANDed, so narrowing is the default.

use crate::sources::format::parse_size;
use crate::sources::models::TorrentItem;

/// How a number token compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Op {
    fn from_prefix(value: &str) -> (Op, &str) {
        for (prefix, op) in [
            (">=", Op::Ge),
            ("<=", Op::Le),
            (">", Op::Gt),
            ("<", Op::Lt),
            ("=", Op::Eq),
        ] {
            if let Some(rest) = value.strip_prefix(prefix) {
                return (op, rest);
            }
        }
        // No operator written: an exact value, which is what
        (Op::Eq, value)
    }

    fn apply(self, left: u64, right: u64) -> bool {
        match self {
            Op::Eq => left == right,
            Op::Lt => left < right,
            Op::Le => left <= right,
            Op::Gt => left > right,
            Op::Ge => left >= right,
        }
    }
}

/// One ANDed token of the filter text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    /// A case-insensitive substring of the row's haystack.
    Word(String),
    /// A case-insensitive substring of the title alone.
    Title(String),
    Source(String),
    Group(String),
    Size(Op, u64),
    Seeds(Op, u32),
}

/// A parsed filter: every term must hold for a row to be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    terms: Vec<Term>,
    /// `negated` marks the term that followed a `-`, one per term.
    negated: Vec<bool>,
}

impl Filter {
    /// Split `text` into whitespace-separated terms.
    pub fn parse(text: &str) -> Self {
        let mut terms = Vec::new();
        let mut negated = Vec::new();
        for token in text.split_whitespace() {
            let (bang, token) = match token.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest),
                _ => (false, token),
            };
            terms.push(Self::term(token));
            negated.push(bang);
        }
        Filter { terms, negated }
    }

    fn term(token: &str) -> Term {
        if let Some((field, value)) = token.split_once(':') {
            match field.to_lowercase().as_str() {
                "src" | "tracker" => return Term::Source(value.to_lowercase()),
                "group" | "cat" => return Term::Group(value.to_lowercase()),
                "size" => {
                    let (op, number) = Op::from_prefix(value);
                    let bytes = parse_size(number);
                    if bytes > 0 || number.trim().is_empty() || number == "0" {
                        return Term::Size(op, bytes);
                    }
                }
                "seeds" => {
                    let (op, number) = Op::from_prefix(value);
                    if let Ok(n) = number.trim().parse::<u32>() {
                        return Term::Seeds(op, n);
                    }
                }
                "title" => return Term::Title(value.to_lowercase()),
                _ => {}
            }
        }
        Term::Word(token.to_lowercase())
    }

    /// Whether `item` survives this filter.
    pub fn matches(&self, item: &TorrentItem) -> bool {
        self.terms.iter().zip(&self.negated).all(|(term, bang)| {
            let hit = match term {
                Term::Word(w) => haystack(item).contains(w),
                Term::Title(w) => item.title.to_lowercase().contains(w),
                Term::Source(id) => item.source.to_lowercase() == **id,
                Term::Group(g) => item
                    .group
                    .is_some_and(|group| group.label().to_lowercase() == **g),
                Term::Size(op, want) => op.apply(item.size_bytes, *want),
                Term::Seeds(op, want) => op.apply(u64::from(item.seeds_n), u64::from(*want)),
            };
            hit != *bang
        })
    }
}

/// The fields a bare word searches: the row's title, size, source and group,
/// plus the two the site states about the release itself -- who announced it and
/// where it filed it. Those two are what a bare `fitgirl` or `switch` is
/// actually asking for, and ext rows carry both from the search answer, so
/// there is nothing extra to read and nothing to fetch.
fn haystack(item: &TorrentItem) -> String {
    format!(
        "{} {} {} {} {} {}",
        item.title,
        item.size,
        item.source,
        item.group.map_or("", |g| g.label()),
        item.uploader,
        item.category,
    )
    .to_lowercase()
}
