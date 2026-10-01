.PHONY: all build release test lint fmt fmt-check doc clean install uninstall run

CARGO ?= cargo
BINDIR ?= $(HOME)/.local/bin
PREFIX ?= /usr/local

all: fmt-check lint test build

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

# Everything CI runs, in the order it runs it.
gate:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test
	$(CARGO) doc --no-deps

lint:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

doc:
	$(CARGO) doc --no-deps

run: build
	./target/debug/doris

install: release
	install -Dm755 target/release/doris $(DESTDIR)$(BINDIR)/doris

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/doris

clean:
	$(CARGO) clean