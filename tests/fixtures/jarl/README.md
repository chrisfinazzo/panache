# Jarl JSON fixtures

`jarl-0.5.0.json`, `jarl-0.6.0.json`, and `jarl-0.7.1.json` are unmodified
captures from the corresponding binaries, collected on 2026-10-21 by running:

```sh
jarl check probe.R --output-format json
```

`probe.R` is the common input. Each release emits one `any_is_na` diagnostic
with a fix. Version 0.5.0 uses `content`/`start`/`end`, 0.6.0 uses
`content`/`range`, and 0.7.1 uses `edits`/`safety`. None of these JSON objects
reports a version, so the parser selects by shape rather than a version guess.
The tests mutate one fix at a time to cover unknown or malformed shapes; those
mutations are synthetic and are not presented as release captures.

`jarl-0.5.0-leading-blank.json` was captured on 2026-10-29 with
`jarl check leading-blank.R --output-format json`. Its diagnostic byte range
correctly covers `x <- 1`, but its reported column is one too large after the
initial blank line. The adapter uses the byte range to preserve the span.

Jarl source: <https://github.com/etiennebacher/jarl>.
