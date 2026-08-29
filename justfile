prefix := env_var_or_default("PREFIX", env_var("HOME") / ".local")
app := "io.github.megalithofficial.gatefold"

build:
    cargo build --release

install: build
    install -Dm755 target/release/gatefold "{{prefix}}/bin/gatefold"
    install -Dm644 crates/gatefold/data/{{app}}.desktop "{{prefix}}/share/applications/{{app}}.desktop"
    install -Dm644 crates/gatefold/data/icons/hicolor/scalable/apps/{{app}}.svg "{{prefix}}/share/icons/hicolor/scalable/apps/{{app}}.svg"

uninstall:
    rm -f "{{prefix}}/bin/gatefold" "{{prefix}}/share/applications/{{app}}.desktop" "{{prefix}}/share/icons/hicolor/scalable/apps/{{app}}.svg"
