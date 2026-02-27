set shell := ["bash", "-c"]

default_target := "aarch64-unknown-linux-gnu"

default:
    @just --list

setup:
    cargo install cargo-binstall
    cargo binstall cargo-tarpaulin cargo-shear cargo-nextest -y

check-deps:
    cargo fmt --all -- --check
    cargo shear --fix
    cargo clippy --all-targets -- -D warnings

test:
    cargo nextest run

coverage:
    cargo tarpaulin --ignore-tests --exclude-files tests/* --out Html
    xdg-open tarpaulin-report.html

build-target target=default_target:
    cargo build --target {{ target }}

run:
    cargo run

ready: check-deps test coverage
    @echo "completed"
