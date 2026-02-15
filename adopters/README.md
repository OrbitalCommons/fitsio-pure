# Adopter Readiness Analysis

Analysis of 25 repos from `potential-adopters.md` that use the `fitsio` crate, evaluated against fitsio-pure's current capabilities.

## Readiness Tiers

### Ready Today (11 repos)
These projects use a subset of fitsio that our compat layer already covers.

| Project | Use Case | FITS Usage |
|---------|----------|------------|
| [qrusthy](qrusthy.md) | QHY camera SDK | Write-only: u16 images + headers |
| [opal](opal.md) | PIC physics simulation | Write-only: f64 histogram images + WCS headers |
| [Star_Tracker](Star_Tracker_Microcontroller.md) | Satellite star tracking | Read-only: f64 table columns from astrometry.net |
| [RapidFits](RapidFits.md) | Desktop FITS viewer | Read-only: f32 primary image + shape |
| [rusty-photon](rusty-photon.md) | PHD2 guider archival | Write-only: u16 guide star images |
| [f2i](f2i.md) | Terminal FITS preview | Read-only: f32 images via `array` feature |
| [fitsrotate_rs](fitsrotate_rs.md) | FITS cube axis rotation | Read/write images + headers via `array` feature |
| [eventide](eventide.md) | Astrophotography processing | Read-only: images + metadata via `array` feature |
| [twinkle](twinkle.md) | Observatory management | Read-only: images + calibration via `array` feature |
| [MARVELpipeline](MARVELpipeline.md) | Spectroscopy pipeline | Read/write images + tables via `array` feature |
| [ccdi](ccdi.md) | CCD camera imaging | Read/write images via `array` feature |

### Probably Ready — Needs Testing (3 repos)
Core operations are covered but specific edge cases need verification.

| Project | Use Case | Risk Area |
|---------|----------|-----------|
| [electra_stacking](electra_stacking.md) | Deep sky image stacking | 3D section reads, unsigned integer round-trip |
| [FastFitsCutter](FastFitsCutter.md) | FITS spatial cutouts | Region I/O API compatibility, generic trait bounds |
| [xisf](xisf.md) | XISF→FITS conversion | u64 BZERO offset handling |

### Partially Ready — Specific Gaps (4 repos)
Would work for most operations but missing specific features.

| Project | Use Case | Blocker |
|---------|----------|---------|
| [catetool](catetool.md) | Eclipse image alignment | Low-level FFI path incompatible (but high-level path works) |
| [dog](dog.md) | Tabular data inspector | Vector column handling (TFORM repeat counts) |
| [serialimage](serialimage.md) | Serializable images | Tile compression support |
| [refimage](refimage.md) | Image data storage | Tile compression support |

### Not Ready — Hard Blockers (5 repos)
Missing fundamental capabilities.

| Project | Use Case | Blockers |
|---------|----------|----------|
| [mwa_hyperdrive](mwa_hyperdrive.md) | Radio telescope calibration | Long strings (CONTINUE), array-in-cell columns, column range reads |
| [mwalib](mwalib.md) | MWA data library | Long strings (CONTINUE), buffer-reuse reads |
| [marlu](marlu.md) | MWA coordinate transforms | Random groups UVFITS format |
| [boom-catalogs](boom-catalogs.md) | Catalog ingestion | `read_col_range()`, `HduInfo::TableInfo`, `fitsio-derive` |
| [FITSWebQL](FITSWebQL.md) | Web FITS viewer | In-memory model can't handle multi-GB files |

### Not Applicable (1 repo)
| Project | Reason |
|---------|--------|
| [citrus](citrus.md) | Dead dependency — fitsio in Cargo.toml but never imported |

### Transitive (1 entry covering 5 repos)
| Project | Via |
|---------|-----|
| [cameraunit stack](cameraunit.md) | Depends on serialimage — migrates automatically |

## Feature Priority Matrix

Based on how many repos each feature would unblock:

| Feature | Status | Unblocks | Repos |
|---------|--------|----------|-------|
| ~~**ndarray integration**~~ | **DONE** | ~~6 repos~~ | ~~f2i, fitsrotate_rs, eventide, twinkle, MARVELpipeline, ccdi~~ |
| **Long string (CONTINUE cards)** | Pending | 2 repos (+3 transitive) | mwa_hyperdrive, mwalib → Birli, mwax_stats |
| **Tile compression** | Pending | 2 repos (+5 transitive) | refimage, serialimage → cameraunit, cameraunit_asi, cameraunit_fli |
| **`read_col_range()`** | Pending | 1 repo | boom-catalogs |
| **Array-in-cell columns** | Pending | 1 repo | mwa_hyperdrive |
| **Streaming/seek-based I/O** | Pending | 1 repo | FITSWebQL |
| **Random groups format** | Pending | 1 repo | marlu |

## Recommended Implementation Order

1. ~~**ndarray feature**~~ — **SHIPPED** via `array` feature flag (#18)
2. **Long string / CONTINUE cards** — unblocks MWA ecosystem (5 repos), important for credibility with radio astronomy community
3. **`read_col_range()`** — targeted fix for boom-catalogs, useful general feature
4. **Tile compression** — large effort but unblocks the sunipkm camera ecosystem (5+ repos)
5. **Streaming I/O** — architectural change, only needed for FITSWebQL's large-file use case
6. **Random groups** — niche format, only marlu needs it and FITS support is optional there
