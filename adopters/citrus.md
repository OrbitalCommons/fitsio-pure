# citrus

**Repository:** [ProsiaLAB/citrus](https://github.com/ProsiaLAB/citrus)
**Category:** Line modelling engine (LIME in Rust)
**FITS Centrality:** None — dead dependency

## What It Does

Rust implementation of LIME (LIne Modelling Engine) for astrophysical line emission modeling.

## FITS Operations Used

None. Despite `fitsio` appearing in Cargo.toml, there are no `use fitsio` statements anywhere in the codebase. This is a dead/unused dependency.

## fitsio-pure Readiness Assessment

### Verdict
**N/A — dead dependency.** No migration needed. citrus should probably remove fitsio from their Cargo.toml entirely.
