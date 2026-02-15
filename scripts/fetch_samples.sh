#!/bin/bash
set -e

DEST="test_data"
mkdir -p "$DEST"

echo "Downloading sample FITS files..."

# Astropy samples
curl -L -o "$DEST/test0.fits" https://github.com/astropy/astropy/raw/main/astropy/io/fits/tests/data/test0.fits
curl -L -o "$DEST/tb.fits" https://github.com/astropy/astropy/raw/main/astropy/io/fits/tests/data/tb.fits
curl -L -o "$DEST/comp.fits" https://github.com/astropy/astropy/raw/main/astropy/io/fits/tests/data/comp.fits

# HEASARC samples
curl -L -o "$DEST/NICMOSn4hk12010_mos.fits" https://fits.gsfc.nasa.gov/samples/NICMOSn4hk12010_mos.fits
curl -L -o "$DEST/IUElwp25637mxlo.fits" https://fits.gsfc.nasa.gov/samples/IUElwp25637mxlo.fits
curl -L -o "$DEST/WFPC2u5780205r_c0fx.fits" https://fits.gsfc.nasa.gov/samples/WFPC2u5780205r_c0fx.fits

echo "Done."
