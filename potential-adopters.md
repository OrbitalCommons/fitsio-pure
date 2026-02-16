# Potential fitsio-pure Adopters

Rust projects currently depending on `fitsio` (the cfitsio C wrapper) that could benefit from switching to `fitsio-pure`.

## Direct Dependencies

- [MWATelescope/mwa_hyperdrive](https://github.com/MWATelescope/mwa_hyperdrive) (132 stars) - Calibration software for the Murchison Widefield Array (MWA) radio telescope
- [MWATelescope/mwalib](https://github.com/MWATelescope/mwalib) (11 stars) - Library to read MWA raw visibilities, voltages, and metadata
- [jvo203/FITSWebQL](https://github.com/jvo203/FITSWebQL) (0 stars) - High-performance FITSWebQL Supercomputer Edition
- [MWATelescope/Marlu](https://github.com/MWATelescope/Marlu) (7 stars) - Coordinate transformations, Jones matrices, etc. for MWA ([issue](https://github.com/MWATelescope/Marlu/issues/39))
- [TrystanScottLambert/dog](https://github.com/TrystanScottLambert/dog) (5 stars) - Like cat but for parquet (also supports FITS) ([issue](https://github.com/TrystanScottLambert/dog/issues/21))
- [TomCreusot/Star_Tracker_Microcontroller](https://github.com/TomCreusot/Star_Tracker_Microcontroller) (5 stars) - Star tracker for spacecraft pointing direction identification
- [tgblackburn/opal](https://github.com/tgblackburn/opal) (4 stars) - Parallel, relativistic 1d3v PIC code written in Rust
- [art-den/electra_stacking](https://github.com/art-den/electra_stacking) (4 stars) - Software for stacking astronomical deep sky images
- [GuoHaoxuan/blink](https://github.com/GuoHaoxuan/blink) (3 stars)
- [wrenby/xisf](https://github.com/wrenby/xisf) (2 stars) - Reader for the XISF astronomy image file format
- [twinkle-astronomy/twinkle](https://github.com/twinkle-astronomy/twinkle) (2 stars)
- [GreatAttractor/catetool](https://github.com/GreatAttractor/catetool) (2 stars) - Image alignment for the Continental-America Telescopic Eclipse Experiment
- [chrischtel/RapidFits](https://github.com/chrischtel/RapidFits) (2 stars)
- [IvS-KULeuven/MARVELpipeline](https://github.com/IvS-KULeuven/MARVELpipeline) (1 star) - Data processing and radial velocity pipeline of the MARVEL spectrograph
- [ProsiaLAB/citrus](https://github.com/ProsiaLAB/citrus) (1 star) - Implementation of LIME (LIne Modelling Engine) in Rust
- [PreSKADataReduction/jm21cma](https://github.com/PreSKADataReduction/jm21cma) (1 star) - Calculate Jones matrix for 21CMA
- [emaadparacha/qrusthy](https://github.com/emaadparacha/qrusthy) (1 star) - Rust wrapper for the QHYCCD SDK for QHY cameras
- [sunipkm/serialimage](https://github.com/sunipkm/serialimage) (1 star) - Serialization for DynamicImage with metadata (archived, fitsio behind feature flag)
- [ivonnyssen/rusty-photon](https://github.com/ivonnyssen/rusty-photon) (0 stars)
- [AlecThomson/fitsrotate_rs](https://github.com/AlecThomson/fitsrotate_rs) (0 stars)
- [sunipkm/refimage](https://github.com/sunipkm/refimage) (0 stars) - Image data storage supporting owned data or references; supports demosaic
- [tikk3r/FastFitsCutter](https://github.com/tikk3r/FastFitsCutter) (0 stars) - Fast no-nonsense FITS cutter
- [Fingel/f2i](https://github.com/Fingel/f2i) (0 stars) - Preview .fits files in the terminal and convert to images
- [yipihey/impress-apps](https://github.com/yipihey/impress-apps) (0 stars)
- [viktorchvatal/ccdi](https://github.com/viktorchvatal/ccdi) (0 stars) - CCD Imaging (Simple) Service
- [CosmicFrontierLabs/meter-sim](https://github.com/CosmicFrontierLabs/meter-sim) (0 stars)
- [szqtc/dpltcubers](https://github.com/szqtc/dpltcubers) (0 stars) - Rust implementation of the DAMPE ltcube calculator
- [asierzapata/eventide](https://github.com/asierzapata/eventide) (0 stars) - Desktop app for astrophotography image processing
- [sunipkm/asicam_rs](https://github.com/sunipkm/asicam_rs) (0 stars) - ZWO ASI Camera SDK v2 Rust API
- [dostergaard/astro-core](https://github.com/dostergaard/astro-core) (0 stars)
- [sunipkm/cameraunit](https://github.com/sunipkm/cameraunit) (0 stars) - Camera interface for image capture in Rust
- [sunipkm/cameraunit_asi](https://github.com/sunipkm/cameraunit_asi) (0 stars) - `cameraunit` implementation for ZWO ASI cameras
- [sunipkm/cameraunit_fli](https://github.com/sunipkm/cameraunit_fli) (0 stars)
- [xorza/Scenarium](https://github.com/xorza/Scenarium) (0 stars)
- [robertoabraham/dragonfly-1](https://github.com/robertoabraham/dragonfly-1) (0 stars)
- [boom-astro/boom-catalogs](https://github.com/boom-astro/boom-catalogs) (0 stars) - Ingest astronomical catalogs into MongoDB for cross-matching

## Transitive Dependencies (via refimage, mwalib, marlu, or birli)

- [MWATelescope/Birli](https://github.com/MWATelescope/Birli) (18 stars) - MWA data pipeline preprocessing (via marlu cfitsio feature)
- [MWATelescope/mwax_stats](https://github.com/MWATelescope/mwax_stats) (0 stars) - MWA correlator statistics (via mwalib and birli)
- [mitbailey/clientserver-rs](https://github.com/mitbailey/clientserver-rs) (0 stars) - Uses refimage with fitsio feature
- [sunipkm/generic_camera_asi](https://github.com/sunipkm/generic_camera_asi) (0 stars) - Uses refimage with fitsio feature
- [sunipkm/comic_uldb_software](https://github.com/sunipkm/comic_uldb_software) (0 stars) - Uses refimage with fitsio feature

## Dev-dependency Only

- [ssmichael1/fitsrs](https://github.com/ssmichael1/fitsrs) (1 star) - FITS reader/writer (uses fitsio in dev-dependencies for comparison testing)

## Plate Solving, Astrometry & Star Tracking

Repos doing plate solving, astrometry, or star pattern matching that would benefit from a pure-Rust FITS library:

- [ssmichael1/satkit](https://github.com/ssmichael1/satkit) (64 stars) - Satellite and Orbital Dynamics Toolkit
- [ssmichael1/tetra3-rs](https://github.com/ssmichael1/tetra3-rs) (7 stars) - Rust implementation of "tetra3" star matching algorithm
- [TomCreusot/Star_Tracker_Microcontroller](https://github.com/TomCreusot/Star_Tracker_Microcontroller) (5 stars) - Star tracker for spacecraft pointing direction identification
- [ssmichael1/starscene](https://github.com/ssmichael1/starscene) (1 star) - Star Scene Generator
- [OrbitalCommons/zodiacal](https://github.com/OrbitalCommons/zodiacal) (0 stars) - Blind astrometry library
- [robertoabraham/cedar-server](https://github.com/robertoabraham/cedar-server) (0 stars) - Plate-solving electronic finder for telescopes

## Related Astronomy Repos by Organization/Maintainer

Additional repos from the same organizations and maintainers that work with astronomical data and could benefit from fitsio-pure.

### MWATelescope

- [mwa_hyperbeam](https://github.com/MWATelescope/mwa_hyperbeam) (6 stars) - MWA beam code
- [giant-squid](https://github.com/MWATelescope/giant-squid) (6 stars) - Alternative MWA ASVO client
- [rust-aoflagger](https://github.com/MWATelescope/rust-aoflagger) (3 stars) - Rust bindings to AOFlagger

### art-den

- [astra_lite](https://github.com/art-den/astra_lite) (52 stars) - AstraLite astrophotography stacking for low-power PCs

### GreatAttractor

- [vidoxide](https://github.com/GreatAttractor/vidoxide) (25 stars) - Video capture for Solar System astrophotography
- [libskry_r](https://github.com/GreatAttractor/libskry_r) (17 stars) - Lucky imaging library (Rust rewrite)
- [ga_image](https://github.com/GreatAttractor/ga_image) (0 stars) - Image handling library
- [vislumino](https://github.com/GreatAttractor/vislumino) (0 stars) - Astronomy Visualization Tools

### tgblackburn

- [ptarmigan](https://github.com/tgblackburn/ptarmigan) (25 stars) - Particle-tracking code for QED interactions

### jvo203

- [fits_web_ql](https://github.com/jvo203/fits_web_ql) (10 stars) - Rust implementation of FITSWebQL
- [FITSWEBQLSE](https://github.com/jvo203/FITSWEBQLSE) (3 stars) - High-performance FITS web viewer
- [test_fits_web_ql](https://github.com/jvo203/test_fits_web_ql) (0 stars) - Rust FITS processing testbed

### boom-astro

- [boom](https://github.com/boom-astro/boom) (8 stars) - Next generation astronomical alert broker
- [flare](https://github.com/boom-astro/flare) (1 star) - BOOM system Rust component
- [boom-api](https://github.com/boom-astro/boom-api) (0 stars) - API for BOOM alert broker

### OrbitalCommons

- [starfield](https://github.com/OrbitalCommons/starfield) (4 stars) - Star field generation
- [zodiacal](https://github.com/OrbitalCommons/zodiacal) (0 stars) - Blind astrometry library

### ProsiaLAB

- [kappa](https://github.com/ProsiaLAB/kappa) (1 star) - Dust opacity calculator (optool in Rust)
- [rendezvous](https://github.com/ProsiaLAB/rendezvous) (1 star) - N-body integration for orbital dynamics
- [elysium](https://github.com/ProsiaLAB/elysium) (1 star) - Moving-mesh magnetohydrodynamics code
- [spectre](https://github.com/ProsiaLAB/spectre) (1 star) - Linelists and molecular transition databases
- [disturbulence](https://github.com/ProsiaLAB/disturbulence) (1 star) - Protoplanetary disk turbulence simulation

### ivonnyssen

- [qhyccd-rs](https://github.com/ivonnyssen/qhyccd-rs) (1 star) - Rust bindings for QHYCCD cameras
- [qhyccd-alpaca](https://github.com/ivonnyssen/qhyccd-alpaca) (1 star) - ASCOM Alpaca server for QHYCCD
- [ascom-alpaca-rs](https://github.com/ivonnyssen/ascom-alpaca-rs) (0 stars) - ASCOM Alpaca API library for astronomy devices

### CosmicFrontierLabs

- [rust-ephem](https://github.com/CosmicFrontierLabs/rust-ephem) (1 star) - Spacecraft Ephemerides and Constraints calculator
- [playerone-sdk-rs](https://github.com/CosmicFrontierLabs/playerone-sdk-rs) (0 stars) - PlayerOne Astronomy Camera SDK bindings

### twinkle-astronomy

- [phd2_exporter](https://github.com/twinkle-astronomy/phd2_exporter) (1 star) - Prometheus exporter for PHD2 guiding
- [indi_exporter](https://github.com/twinkle-astronomy/indi_exporter) (0 stars) - Prometheus exporter for INDI

### ssmichael1

- [fitsview](https://github.com/ssmichael1/fitsview) (0 stars) - FITS Viewer
- [camera](https://github.com/ssmichael1/camera) (1 star) - Rust camera interface bindings
- [svbony](https://github.com/ssmichael1/svbony) (0 stars) - Rust SVBony Camera SDK bindings

### chrischtel

- [rsfitsio](https://github.com/chrischtel/rsfitsio) (1 star) - Rust re-write of cfitsio
- [zfitsio](https://github.com/chrischtel/zfitsio) (1 star) - Zig wrapper/bindings for CFITSIO
- [RapidRAW](https://github.com/chrischtel/RapidRAW) (0 stars) - GPU-accelerated RAW image editor

### tikk3r

- [H5O3](https://github.com/tikk3r/H5O3) (0 stars) - LOFAR H5parm solution tables
- [lofar-h5plot-rs](https://github.com/tikk3r/lofar-h5plot-rs) (0 stars) - LOFAR calibration solution visualization

### PreSKADataReduction

- [oskar_gain](https://github.com/PreSKADataReduction/oskar_gain) (0 stars) - Rust telescope simulation tool
- [lds](https://github.com/PreSKADataReduction/lds) (0 stars) - Rust data processing
- [dbf_beam_simulator](https://github.com/PreSKADataReduction/dbf_beam_simulator) (0 stars) - Digital beamforming simulator

### TrystanScottLambert

- [astroxide](https://github.com/TrystanScottLambert/astroxide) (0 stars) - Astronomy utils for Rust
- [cosmoxide](https://github.com/TrystanScottLambert/cosmoxide) (0 stars) - Cosmology utils for Rust
