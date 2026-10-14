SHELL := /bin/sh
.DEFAULT_GOAL := help

CARGO ?= cargo
BOLTFFI ?= boltffi
TRIVY ?= trivy
DOCKER ?= docker
IMAGE ?= ac-man:local
DOCKER_OUTPUT ?= dist/docker

.PHONY: help fmt fmt-check check test clippy cli app bindings audit trivy security ci licenses docker-build docker-test docker-export trivy-image

help:
	@printf '%s\n' \
	  'make cli            Build the release CLI in target/release/' \
	  'make app            Build the macOS menu-bar app and CLI in dist/' \
	  'make bindings       Generate/package Swift bindings with BoltFFI' \
	  'make fmt            Format Rust source' \
	  'make fmt-check      Check Rust formatting without changing files' \
	  'make check          Check all Rust targets' \
	  'make test           Run all Rust tests (no device credentials needed)' \
	  'make clippy         Run Clippy with warnings treated as errors' \
	  'make ci             Run formatting, Clippy and tests' \
	  'make audit          Check Cargo.lock against RustSec (cargo-audit)' \
	  'make trivy          Scan source for vulnerabilities, secrets and misconfiguration' \
	  'make security       Run cargo-audit and Trivy' \
	  'make licenses       Generate Rust dependency notices (cargo-about)' \
	  'make docker-test    Run Rust tests in the Linux builder' \
	  'make docker-export  Export the Linux CLI to dist/docker/' \
	  'make docker-build   Build an optional non-root runtime image' \
	  'make trivy-image    Scan the built runtime image'

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

check:
	$(CARGO) check --locked --all-targets

test:
	$(CARGO) test --locked --all-targets

clippy:
	$(CARGO) clippy --locked --all-targets -- -D warnings

cli:
	$(CARGO) build --locked --release --bin ac-man

app:
	./build.sh

bindings:
	$(BOLTFFI) pack apple --release

audit:
	$(CARGO) audit --file Cargo.lock

trivy:
	$(TRIVY) fs --scanners vuln,secret,misconfig --severity HIGH,CRITICAL --exit-code 1 \
	  --skip-dirs .git --skip-dirs target --skip-dirs .build --skip-dirs generated --skip-dirs dist \
	  --skip-dirs data --skip-dirs work --skip-dirs .venv --skip-dirs '**/*.app' .

security: audit trivy

ci: fmt-check clippy test

licenses:
	mkdir -p licenses
	$(CARGO) about generate --locked --fail -o licenses/DEPENDENCIES.html about.hbs

docker-test:
	$(DOCKER) buildx build --target test --output type=cacheonly --progress plain .

docker-export:
	$(DOCKER) buildx build --target cli --output 'type=local,dest=$(DOCKER_OUTPUT)' .

docker-build: licenses
	$(DOCKER) buildx build --target runtime --load --tag '$(IMAGE)' .

trivy-image:
	$(TRIVY) image --severity HIGH,CRITICAL --exit-code 1 '$(IMAGE)'
