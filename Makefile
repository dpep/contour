# Homebrew's rustup is keg-only, so cargo may not be on PATH. Appending rather
# than prepending leaves an existing toolchain in charge.
export PATH := $(PATH):/opt/homebrew/opt/rustup/bin

.PHONY: check build release dogfood

## the commit gate: fmt, clippy, tests
check:
	@script/check.sh

build:
	@cargo build

release:
	@cargo build --release

## feel the tool on real code. REPO= picks the target.
REPO ?= ~/code/lib/ruby/rails
dogfood: release
	@CONTOUR_DB=/tmp/contour-dogfood.db ./target/release/contour index $(REPO)
	@CONTOUR_DB=/tmp/contour-dogfood.db ./target/release/contour --status
