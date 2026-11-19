# Flathub submission

`com.felixadr.Calen.yml` here is the manifest for Flathub. It matches
`../com.felixadr.Calen.yml` except that it builds a release tag from git, and its
`x-checker-data` lets Flathub's bot open update PRs when new `v*` tags appear.

## Steps

1. Tag the release (`v0.1.7`) and replace `COMMIT_OF_v0.1.7` with that tag's commit.
2. Fork `flathub/flathub`. On a branch from `new-pr`, add `com.felixadr.Calen.yml`
   and `../cargo-sources.json`, then open a PR against `new-pr`.
3. During review, verify felixadr.com: Flathub shows a token to serve at
   `https://felixadr.com/.well-known/org.flathub.VerifiedApps.txt`.
4. Once it's accepted, releases are updated in `flathub/com.felixadr.Calen`.
   Each update needs the new tag and commit, plus `cargo-sources.json` if `Cargo.lock`
   changed.

The local `flatpak-builder-lint repo` errors `appstream-external-screenshot-url` and
`appstream-screenshots-not-mirrored-in-ostree` are expected. Flathub's builders mirror the
screenshots themselves.
