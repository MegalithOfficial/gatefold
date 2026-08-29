<!--
Use a Conventional Commit title, for example:
feat(lyrics): add a romanization toggle
fix(player): reload the current track when play is pressed after the queue stopped
-->

## Summary

<!-- What changed? Keep this to what reviewers need to understand the behavior. -->

## Why

<!-- What problem does this solve, and why is this the right approach for Gatefold? -->

## Testing

<!-- Commands and manual workflows. Say which platform and audio setup you ran on. -->

- [ ] `cargo +nightly fmt`
- [ ] `cargo test -p gatefold-core`

## Visual changes

<!-- Before and after screenshots or a short recording. Remove this section when nothing visible changed. -->

## Checklist

- [ ] I reviewed the complete diff and removed unrelated changes.
- [ ] I tested the affected workflow in the running application.
- [ ] I followed the existing crate split: Spotify, playback and data in `gatefold-core`, GTK in `gatefold`.
- [ ] I matched the surrounding code and stylesheets rather than adding a separate style.
- [ ] I added or updated tests where the changed behavior can be tested reliably.
- [ ] I updated the README when setup or behavior changed.
- [ ] I did not include credentials, cached data, build output or debug code.
- [ ] I directed and reviewed this work; it was not blindly or entirely delegated to AI.
- [ ] I disclosed any material use of generated code in the summary.
