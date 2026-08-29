# Contributing to Gatefold

Gatefold is a Spotify client. Changes can touch account credentials, the audio
device, cached data and a library the user cares about. A pull request must be
based on the code that is here, not on how a typical GTK or librespot
application might work.

Bug fixes and small, self-contained improvements can go straight to a pull
request. Open an issue before starting a large feature, a new page, a change to
stored data, or anything that talks to a Spotify endpoint the code does not use
yet.

## Running the project

Install Rust 1.88 or newer, GTK 4.22 and libadwaita 1.9 (see the README for
per-platform packages). From the repository root:

```sh
cargo run
```

Useful while developing:

- `RUST_LOG=debug cargo run` for full logs.
- `cargo run -- --reauthenticate` to drop the cached session and sign in again.
- `GATEFOLD_BACKEND=pipe` (or any librespot backend name) to bypass the built-in
  audio sink.
- Cached sessions, tokens and images live in `~/.cache/gatefold`; settings in
  `~/.config/gatefold/settings.json`.

Linux is the primary development platform. If a change is intended for Windows
or macOS, say which platform you actually ran it on.

## Two crates, one boundary

**`gatefold-core` owns Spotify, playback and data.** The librespot session,
sign-in, the audio sink, the queue, metadata, search, lyrics, caches and
settings live there. It has no GTK dependency and must stay that way: the
same crate is meant to drive the Android front end later, so keep its public
surface plain (owned strings, simple structs and enums, channels and
`Arc`s, no GTK types).

**`gatefold` owns the window.** `components/` are the shell (rack, topbar,
deck) and `pages/` are the pages, each a relm4 component with its own
`style.css` composed by `css.rs`. The UI reads state from core and sends
requests to it; it does not talk to librespot, reqwest or the audio device
directly, and it does not reimplement rules that decide what gets played,
cached or stored.

Do not move logic into the UI merely because the data is already there. If a
calculation is based on Spotify data or could be needed by another page, do
it in core and return the shaped result.

## Playback

`player.rs` is the only caller of librespot's `Player`, and `sink.rs` is the
only thing that talks to the audio device. The UI drives playback through
`Playback` and follows it through `Playback::events()`; it never calls the
player itself. State that the deck shows (playing, position, queue, shuffle,
repeat) is confirmed by events, not assumed after a click.

Seeks, track changes and reconnects flush the sink deliberately. If you change
how audio reaches the device, test pause, seek, skip, natural track end,
gapless transitions and a dropped connection, and say so in the pull request.

## Network and credentials

Requests go through `net.rs`: `web_api` and `partner_api` for Spotify (plus
the spclient on the session), `public_api` for third-party services, `page`
for plain fetches. Those
helpers own the user agent, retries, rate-limit handling and the global
concurrency limit. Do not create a reqwest client for one feature or bypass
the helpers to save a line.

Spotify's undocumented endpoints change. Keep every persisted-query hash and
endpoint constant in one place with the rest, and prefer batched metadata
calls to loops of single lookups.

Tokens and credentials belong in the cache files that `session.rs` and
`auth.rs` already manage. Never log them, never put them in settings, and
never send them to a third-party lyrics or image service.

## Interface

Match the surrounding page. Colours come from the palette tokens
(`@surface`, `@on_surface`, `@primary`, ...) derived from the cover; component
stylesheets use tokens or `alpha()` of tokens, never hex literals. Motion uses
the existing easing family and durations. Icons are bundled SVGs in
`crates/gatefold/data/icons` and must be filled outlines, not strokes — GTK
recolours symbolic icons by forcing the fill.

Show loading, empty and failure states where an operation has them, with
skeletons that match the geometry of what they stand in for. Include
screenshots or a short recording in the pull request.

The source has no comments. Names carry the meaning; if something needs
explaining, the pull request description is the place.

## Tests and checks

```sh
cargo +nightly fmt
cargo test -p gatefold-core
```

Formatting needs nightly for the import grouping in `rustfmt.toml`; stable
silently skips it. Tests live beside the implementation in core and cover
parsing, timing, caches and ranking. There is no UI test runner; UI changes
need a manual pass in the running application. State what you tested; do not
imply coverage you did not perform.

## Pull requests

Keep the change focused. Do not mix a feature with unrelated cleanup,
formatting or dependency updates. Review the complete diff for credentials,
cached data, build output, debug code and scratch files.

**Commit messages follow the Conventional Commits format**, subject only unless
a body is genuinely needed: `feat(lyrics): ...`, `fix(player): ...`,
`build: ...`. Pull request titles use the same format. Describe the actual
change in the imperative mood.

## AI-assisted contributions

**AI-assisted coding is allowed. Blind vibecoding is not.**

An AI tool may help investigate code, suggest a focused implementation,
explain an API, or review work the contributor is actively directing. It must
not replace the contributor's understanding or judgment. Do not hand an entire
issue to a tool, prompt it until the project compiles, and submit the result
without doing the engineering work yourself. Passing CI does not prove that a
change fits Gatefold or handles failure outside the happy path.

The contributor must inspect the relevant code, choose the approach, review
every generated change, test the real workflow, and be able to explain and
maintain the result. **Material use of generated code must be disclosed in the
pull request.** Small autocomplete, spelling and formatting assistance does not
need disclosure.

A pull request may be closed without further review when the author cannot
explain the code, delegates the whole implementation to AI, relies on invented
APIs, introduces broad generated churn, or passes review comments back to a
tool without understanding the response.

## Reporting bugs

Search existing issues, then use the
[bug report form](https://github.com/MegalithOfficial/gatefold/issues/new?template=bug_report.yml).
Include what you did, what happened and what should have happened, the
shortest sequence that reproduces it, the commit you ran, your OS, desktop and
GTK version, and your audio setup when playback is involved.

Attach the relevant part of the log from `RUST_LOG=debug cargo run`. Remove
access tokens, account identifiers, usernames and private paths before
posting; nothing is redacted automatically.
