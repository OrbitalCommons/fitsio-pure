# Potential fitsio-pure Adopters

Rust projects currently depending on `fitsio` (the cfitsio C wrapper) that could benefit from switching to `fitsio-pure`.

## Direct Dependencies

- [MWATelescope/mwa_hyperdrive](https://github.com/MWATelescope/mwa_hyperdrive) - Calibration software for the Murchison Widefield Array (MWA) radio telescope
- [TrystanScottLambert/dog](https://github.com/TrystanScottLambert/dog) - Like cat but for parquet (also supports FITS)
- [tgblackburn/opal](https://github.com/tgblackburn/opal) - Parallel, relativistic 1d3v PIC code written in Rust
- [TomCreusot/Star_Tracker_Microcontroller](https://github.com/TomCreusot/Star_Tracker_Microcontroller) - Star tracker for spacecraft pointing direction identification
- [IvS-KULeuven/MARVELpipeline](https://github.com/IvS-KULeuven/MARVELpipeline) - Data processing and radial velocity pipeline of the MARVEL spectrograph
- [GreatAttractor/catetool](https://github.com/GreatAttractor/catetool) - Image alignment for the Continental-America Telescopic Eclipse Experiment
- [ProsiaLAB/citrus](https://github.com/ProsiaLAB/citrus) - Implementation of LIME (LIne Modelling Engine) in Rust
- [ivonnyssen/rusty-photon](https://github.com/ivonnyssen/rusty-photon)
- [AlecThomson/fitsrotate_rs](https://github.com/AlecThomson/fitsrotate_rs)
- [sunipkm/refimage](https://github.com/sunipkm/refimage) - Image data storage supporting owned data or references; supports demosaic
- [tikk3r/FastFitsCutter](https://github.com/tikk3r/FastFitsCutter) - Fast no-nonsense FITS cutter
- [Fingel/f2i](https://github.com/Fingel/f2i) - Preview .fits files in the terminal and convert to images
- [PreSKADataReduction/jm21cma](https://github.com/PreSKADataReduction/jm21cma) - Calculate Jones matrix for 21CMA
- [yipihey/impress-apps](https://github.com/yipihey/impress-apps)
- [viktorchvatal/ccdi](https://github.com/viktorchvatal/ccdi) - CCD Imaging (Simple) Service
- [CosmicFrontierLabs/meter-sim](https://github.com/CosmicFrontierLabs/meter-sim)
- [szqtc/dpltcubers](https://github.com/szqtc/dpltcubers) - Rust implementation of the DAMPE ltcube calculator
- [asierzapata/eventide](https://github.com/asierzapata/eventide) - Desktop app for astrophotography image processing
- [MWATelescope/Marlu](https://github.com/MWATelescope/Marlu) - Coordinate transformations, Jones matrices, etc. for MWA ([issue](https://github.com/MWATelescope/Marlu/issues/39))
- [chrischtel/RapidFits](https://github.com/chrischtel/RapidFits)
- [sunipkm/asicam_rs](https://github.com/sunipkm/asicam_rs) - ZWO ASI Camera SDK v2 Rust API
- [dostergaard/astro-core](https://github.com/dostergaard/astro-core)
- [sunipkm/cameraunit](https://github.com/sunipkm/cameraunit) - Camera interface for image capture in Rust
- [sunipkm/cameraunit_asi](https://github.com/sunipkm/cameraunit_asi) - `cameraunit` implementation for ZWO ASI cameras
- [sunipkm/cameraunit_fli](https://github.com/sunipkm/cameraunit_fli)
- [wrenby/xisf](https://github.com/wrenby/xisf) - Reader for the XISF astronomy image file format
- [xorza/Scenarium](https://github.com/xorza/Scenarium)
- [twinkle-astronomy/twinkle](https://github.com/twinkle-astronomy/twinkle)
- [MWATelescope/mwalib](https://github.com/MWATelescope/mwalib) - Library to read MWA raw visibilities, voltages, and metadata
- [GuoHaoxuan/blink](https://github.com/GuoHaoxuan/blink)
- [art-den/electra_stacking](https://github.com/art-den/electra_stacking) - Software for stacking astronomical deep sky images
- [jvo203/FITSWebQL](https://github.com/jvo203/FITSWebQL) - High-performance FITSWebQL Supercomputer Edition
- [robertoabraham/dragonfly-1](https://github.com/robertoabraham/dragonfly-1)
- [boom-astro/boom-catalogs](https://github.com/boom-astro/boom-catalogs) - Ingest astronomical catalogs into MongoDB for cross-matching
- [sunipkm/serialimage](https://github.com/sunipkm/serialimage) - Serialization for DynamicImage with metadata (archived, fitsio behind feature flag)
- [emaadparacha/qrusthy](https://github.com/emaadparacha/qrusthy) - Rust wrapper for the QHYCCD SDK for QHY cameras

## Transitive Dependencies (via refimage, mwalib, marlu, or birli)

- [mitbailey/clientserver-rs](https://github.com/mitbailey/clientserver-rs) - Uses refimage with fitsio feature
- [sunipkm/generic_camera_asi](https://github.com/sunipkm/generic_camera_asi) - Uses refimage with fitsio feature
- [sunipkm/comic_uldb_software](https://github.com/sunipkm/comic_uldb_software) - Uses refimage with fitsio feature
- [MWATelescope/Birli](https://github.com/MWATelescope/Birli) - MWA data pipeline preprocessing (via marlu cfitsio feature)
- [MWATelescope/mwax_stats](https://github.com/MWATelescope/mwax_stats) - MWA correlator statistics (via mwalib and birli)

## Dev-dependency Only

- [ssmichael1/fitsrs](https://github.com/ssmichael1/fitsrs) - FITS reader/writer (uses fitsio in dev-dependencies for comparison testing)

## Plate Solving, Astrometry & Star Tracking

Repos doing plate solving, astrometry, or star pattern matching that would benefit from a pure-Rust FITS library:

- [OrbitalCommons/zodiacal](https://github.com/OrbitalCommons/zodiacal) - Blind astrometry library
- [ssmichael1/tetra3-rs](https://github.com/ssmichael1/tetra3-rs) - Rust implementation of "tetra3" star matching algorithm
- [TomCreusot/Star_Tracker_Microcontroller](https://github.com/TomCreusot/Star_Tracker_Microcontroller) - Star tracker for spacecraft pointing direction identification
- [robertoabraham/cedar-server](https://github.com/robertoabraham/cedar-server) - Plate-solving electronic finder for telescopes
- [ssmichael1/satkit](https://github.com/ssmichael1/satkit) - Satellite and Orbital Dynamics Toolkit
- [ssmichael1/starscene](https://github.com/ssmichael1/starscene) - Star Scene Generator

## Related Astronomy Repos by Organization/Maintainer

Additional repos from the same organizations and maintainers that work with astronomical data and could benefit from fitsio-pure.

### MWATelescope

- [mwa_hyperbeam](https://github.com/MWATelescope/mwa_hyperbeam) - MWA beam code
- [giant-squid](https://github.com/MWATelescope/giant-squid) - Alternative MWA ASVO client
- [rust-aoflagger](https://github.com/MWATelescope/rust-aoflagger) - Rust bindings to AOFlagger

### ProsiaLAB

- [kappa](https://github.com/ProsiaLAB/kappa) - Dust opacity calculator (optool in Rust)
- [rendezvous](https://github.com/ProsiaLAB/rendezvous) - N-body integration for orbital dynamics
- [elysium](https://github.com/ProsiaLAB/elysium) - Moving-mesh magnetohydrodynamics code
- [spectre](https://github.com/ProsiaLAB/spectre) - Linelists and molecular transition databases
- [disturbulence](https://github.com/ProsiaLAB/disturbulence) - Protoplanetary disk turbulence simulation

### PreSKADataReduction

- [oskar_gain](https://github.com/PreSKADataReduction/oskar_gain) - Rust telescope simulation tool
- [lds](https://github.com/PreSKADataReduction/lds) - Rust data processing
- [dbf_beam_simulator](https://github.com/PreSKADataReduction/dbf_beam_simulator) - Digital beamforming simulator

### boom-astro

- [boom](https://github.com/boom-astro/boom) - Next generation astronomical alert broker
- [boom-api](https://github.com/boom-astro/boom-api) - API for BOOM alert broker
- [flare](https://github.com/boom-astro/flare) - BOOM system Rust component

### twinkle-astronomy

- [phd2_exporter](https://github.com/twinkle-astronomy/phd2_exporter) - Prometheus exporter for PHD2 guiding
- [indi_exporter](https://github.com/twinkle-astronomy/indi_exporter) - Prometheus exporter for INDI

### CosmicFrontierLabs

- [rust-ephem](https://github.com/CosmicFrontierLabs/rust-ephem) - Spacecraft Ephemerides and Constraints calculator
- [playerone-sdk-rs](https://github.com/CosmicFrontierLabs/playerone-sdk-rs) - PlayerOne Astronomy Camera SDK bindings

### OrbitalCommons

- [zodiacal](https://github.com/OrbitalCommons/zodiacal) - Blind astrometry library
- [starfield](https://github.com/OrbitalCommons/starfield) - Star field generation

### Individual Maintainers

**GreatAttractor:**
- [vidoxide](https://github.com/GreatAttractor/vidoxide) - Video capture for Solar System astrophotography
- [ga_image](https://github.com/GreatAttractor/ga_image) - Image handling library
- [vislumino](https://github.com/GreatAttractor/vislumino) - Astronomy Visualization Tools
- [libskry_r](https://github.com/GreatAttractor/libskry_r) - Lucky imaging library (Rust rewrite)

**ivonnyssen:**
- [qhyccd-rs](https://github.com/ivonnyssen/qhyccd-rs) - Rust bindings for QHYCCD cameras
- [ascom-alpaca-rs](https://github.com/ivonnyssen/ascom-alpaca-rs) - ASCOM Alpaca API library for astronomy devices
- [qhyccd-alpaca](https://github.com/ivonnyssen/qhyccd-alpaca) - ASCOM Alpaca server for QHYCCD

**tikk3r:**
- [H5O3](https://github.com/tikk3r/H5O3) - LOFAR H5parm solution tables
- [lofar-h5plot-rs](https://github.com/tikk3r/lofar-h5plot-rs) - LOFAR calibration solution visualization

**art-den:**
- [astra_lite](https://github.com/art-den/astra_lite) - AstraLite astrophotography stacking for low-power PCs

**jvo203:**
- [FITSWEBQLSE](https://github.com/jvo203/FITSWEBQLSE) - High-performance FITS web viewer
- [fits_web_ql](https://github.com/jvo203/fits_web_ql) - Rust implementation of FITSWebQL
- [test_fits_web_ql](https://github.com/jvo203/test_fits_web_ql) - Rust FITS processing testbed

**ssmichael1:**
- [fitsview](https://github.com/ssmichael1/fitsview) - FITS Viewer
- [camera](https://github.com/ssmichael1/camera) - Rust camera interface bindings
- [svbony](https://github.com/ssmichael1/svbony) - Rust SVBony Camera SDK bindings

**tgblackburn:**
- [ptarmigan](https://github.com/tgblackburn/ptarmigan) - Particle-tracking code for QED interactions

**chrischtel:**
- [RapidRAW](https://github.com/chrischtel/RapidRAW) - GPU-accelerated RAW image editor
- [zfitsio](https://github.com/chrischtel/zfitsio) - Zig wrapper/bindings for CFITSIO
- [rsfitsio](https://github.com/chrischtel/rsfitsio) - Rust re-write of cfitsio

**TrystanScottLambert:**
- [astroxide](https://github.com/TrystanScottLambert/astroxide) - Astronomy utils for Rust
- [cosmoxide](https://github.com/TrystanScottLambert/cosmoxide) - Cosmology utils for Rust
