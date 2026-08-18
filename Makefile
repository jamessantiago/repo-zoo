# repo-zoo — code project launcher
# Targets: all, build, run, dev, test, lint, fmt, fmt-check, check, clean,
#          install, uninstall, win-setup

CARGO ?= cargo

.PHONY: all build run dev test lint fmt fmt-check check clean install uninstall win-setup

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

# Build the Windows binary and package it as a setup.exe with NSIS (works from
# Linux; requires `makensis`).
win-setup:
	$(CARGO) build --release --target x86_64-pc-windows-gnu
	mkdir -p windows/Output
	makensis -DEXE=../target/x86_64-pc-windows-gnu/release/repo-zoo.exe windows/repo-zoo.nsi

clean:
	$(CARGO) clean