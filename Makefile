.PHONY: all build release test lint fmt fmt-check doc clean install uninstall run test-ext xpi

CARGO ?= cargo
PYTHON ?= python3
BINDIR ?= $(HOME)/.local/bin
PREFIX ?= /usr/local

all: fmt-check lint test build

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

# The add-on's own logic, under node: it is the part with a bug in it (which
# selector on which site), and it is not Rust, so `cargo test` cannot see it.
# Skipped with a note rather than failing when there is no node -- a gate
# that cannot run on a machine is a gate that gets skipped everywhere.
# The installable file: one zip, written by the same packer the live check
# loads, so the file the user installs and the file that was tested are one
# file and not two that happen to agree today.
xpi:
	$(PYTHON) browser-extension/pack.py

test-ext:
	@if command -v node >/dev/null 2>&1; then \
		node browser-extension/test/title_test.mjs; \
	else \
		echo "note: no node, skipping the add-on's tests"; \
	fi

# Everything CI runs, in the order it runs it.
gate:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test
	$(MAKE) test-ext
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