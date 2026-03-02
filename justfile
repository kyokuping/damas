set shell := ["bash", "-c"]
set windows-shell := ["powershell.exe", "-Command"]

default_target := "aarch64-unknown-linux-gnu"
host_triple := if os() == "windows" { "x86_64-pc-windows-msvc" } else { `rustc -vV | sed -n 's/host: //p'` }
opener := if os() == "windows" { "powershell.exe Start-Process" } else if os() == "macos" { "open" } else if os() == "linux" { "xdg-open" } else { error("Unsuportted OS: " + os()) }
project_name := "damas"
executable := if os() == "windows" { project_name + ".exe" } else { project_name }

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
    @{{ opener }} tarpaulin-report.html 2>/dev/null || echo "report generated: tarpaulin-report.html (Please open it manually)"

run:
    cargo run

pre-ready:
    @{{ if os() == "macos" { "just ready-macos" } else if os() == "linux" { "just ready-linux" } else if os() == "windows" { "just ready-windows" } else { "true" } }}

ready: check-deps test coverage
    @echo "ready completed"

build target=default_target: check-deps pre-ready
    @echo "Building for target: {{ target }}"
    cargo build --target {{ target }}

build-cross target=default_target: check-deps pre-ready
    cargo install cross
    @which cross > /dev/null || cargo install cross
    cross build --target {{ target }}

release target=default_target: ready
    @echo "💻 Host triple: {{ host_triple }}"
    @echo "🎯 Target triple: {{ target }}"
    @{{ if target == host_triple { "just build " + target } else { "just build-cross " + target } }}
    @echo "Building for release (Target: {{ target }})"
    cargo build --release --target {{ target }}
    -@ls -lh target/{{ target }}/release/{{ executable }} | awk '{print "Build Success 📦 Binary size:", $5}' || echo "⚠️  Size check skipped (awk not found or file missing)"
    @echo "✅ Release build completed"

[private]
ready-macos:
    @echo "🍎 Checking macOS environment..."
    @xcode-select -p > /dev/null || (echo "Xcode CLT not found! Run: xcode-select --install"; exit 1)

[private]
ready-linux:
    @echo "🐧 Checking Linux environment..."
    @which mold > /dev/null || (echo "mold not found! Please install it with your package manager."; exit 1)

[private]
ready-windows:
    @echo "🪟 Checking Windows environment...(Only support x86_64-pc-windows-msvc)"
    @just check-msvc
    @just check-win-sdk
    @just check-os-compat

[private]
check-msvc:
    @where.exe link > $null 2>&1 || (echo "X MSVC Linker (link.exe) not found. Please install Visual Studio Build Tools."; exit 1)
    @echo "-> MSVC Linker: OK"

[private]
check-win-sdk:
    @$sdkPath = "C:\Program Files (x86)\Windows Kits\10\Include"; \
    if (Test-Path $sdkPath) { \
        $v = Get-ChildItem $sdkPath | Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty Name; \
        echo "-> Windows SDK ($v): OK"; \
    } else { \
        echo "❌ Windows SDK not found. Install it via winget or VS Installer."; exit 1; \
    }

[private]
check-os-compat:
    @$os = [System.Environment]::OSVersion.Version; \
    if ($os.Build -lt 17134) { \
        echo "⚠️  Warning: Low Windows build number ($($os.Build)). compio might be unstable."; \
    } else { \
        echo "-> OS Build ($($os.Build)): OK"; \
    }
