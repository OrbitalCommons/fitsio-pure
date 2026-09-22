# fitsio-pure

A pure Rust FITS read/write library with a compatibility module that mirrors the `fitsio` crate's API.

## Architecture

- Core library: pure Rust FITS reader/writer (no C dependencies)
- `compat` module: mirrors the call signature of the `fitsio` crate for drop-in replacement

## Reference Materials

Reference materials live in `reference/` (gitignored). To populate them:

1. **rust-fitsio** - Existing Rust FITS library (wraps cfitsio). The `compat` module should match this crate's API:
   ```
   git clone https://github.com/simonrw/rust-fitsio reference/rust-fitsio
   ```

2. **cfitsio** - The canonical C FITS I/O library (upstream of rust-fitsio):
   ```
   git clone https://github.com/HEASARC/cfitsio reference/cfitsio
   ```

3. **FITS Standard 3.0 specification** (PDF + grep-friendly text):
   ```
   curl -L -o reference/fits_standard30aa.pdf "https://fits.gsfc.nasa.gov/standard30/fits_standard30aa.pdf"
   pdftotext -layout reference/fits_standard30aa.pdf reference/fits_standard30aa.txt
   ```

These files should never be checked into the repo.

## Releasing

- Releases are automated: merging a version change to `main` publishes it. `.github/workflows/publish.yml` publishes every crate whose `name@version` is not yet on crates.io, then tags `vX.Y.Z` and creates the GitHub release from that version's `CHANGELOG.md` section.
- To release, bump `version` in `crates/fitsio-pure/Cargo.toml`, add a `## X.Y.Z` section to `CHANGELOG.md`, and commit the updated `Cargo.lock`, all in the same PR. The `Release Check` workflow fails the PR if the changelog entry is missing or the crate doesn't package.
- Never run `cargo publish` by hand. If a publish fails after merge, fix the cause and re-run the Publish workflow (`gh workflow run publish.yml`).
- Publishing uses the `CARGO_REGISTRY_TOKEN` repository secret: a crates.io token with the `publish-update` scope, restricted to `fitsio-pure`.
