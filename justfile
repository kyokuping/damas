set shell := ["bash", "-c"]

default_target := "aarch64-unknown-linux-gnu"
targets := "x86_64-pc-windows-msvc \
    x86_64-unknown-linux-gnu \
    aarch64-unknown-linux-gnu \
    aarch64-apple-darwin"

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
    @rustup target add {{ target }} > /dev/null 2>&1
    cargo build --target {{ target }}

build-targets *target_list:
    @for target in {{ target_list }}; do \
        echo "🛠️ Building for $target..."; \
        just build-target $target; \
    done

build-all:
    @for target in {{ targets }}; do \
        echo "🛠️ Building for $target..."; \
        just build-target $target; \
    done

run:
    cargo run

ready: check-deps test coverage
    @echo "completed"
