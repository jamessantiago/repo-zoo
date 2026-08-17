# repo-zoo — code project launcher
# Targets: all, build, run, dev, test, lint, fmt, fmt-check, check, clean,
#          install, uninstall

CARGO ?= cargo

.PHONY: all build run dev test lint fmt fmt-check check clean install uninstall

all: build

build:
	$(CARGO) build --release

dev:
	$(CARGO) build

run:
	$(CARGO) run

test:
	$(CARGO) test

lint:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

check:
	$(CARGO) check

install:
	./scripts/install.sh

uninstall:
	./scripts/install.sh --uninstall

clean:
	$(CARGO) clean