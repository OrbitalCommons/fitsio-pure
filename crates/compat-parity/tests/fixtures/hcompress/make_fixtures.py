"""Write HCOMPRESS_1 and PLIO_1 fixtures with astropy, plus astropy's decode
of each as little-endian f64 (`<name>.expected`)."""
import sys, os, shutil, numpy as np
from astropy.io import fits

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
h, w = 23, 57
y, x = np.mgrid[0:h, 0:w]
rng = np.random.default_rng(7)
smooth_img = 1000 + 300 * np.sin(x / 9.0) * np.cos(y / 6.0) + rng.normal(0, 12, (h, w))

def write(name, data, **kw):
    path = os.path.join(out, name + ".fits")
    hdu = fits.CompImageHDU(data=data, **kw)
    fits.HDUList([fits.PrimaryHDU(), hdu]).writeto(path, overwrite=True)
    with fits.open(path) as f:
        decoded = np.asarray(f[1].data, dtype=np.float64)
    decoded.astype("<f8").tofile(os.path.join(out, name + ".expected"))
    print(name, data.dtype, kw)

i16 = (smooth_img - 1000).astype(np.int16)
write("hc_i16_lossless", i16, compression_type="HCOMPRESS_1", hcomp_scale=0)
write("hc_i16_scale4", i16, compression_type="HCOMPRESS_1", hcomp_scale=4)
write("hc_i16_scale4_smooth", i16, compression_type="HCOMPRESS_1", hcomp_scale=4, hcomp_smooth=True)
write("hc_i16_tiles_8x16", i16, compression_type="HCOMPRESS_1", hcomp_scale=2, tile_shape=(8, 16))
write("hc_u8_lossless", (smooth_img / 8).astype(np.uint8), compression_type="HCOMPRESS_1")
write("hc_u16_scale2_smooth", (smooth_img * 20).astype(np.uint16), compression_type="HCOMPRESS_1", hcomp_scale=2, hcomp_smooth=True)
write("hc_i32_scale8", (smooth_img * 1e5).astype(np.int32), compression_type="HCOMPRESS_1", hcomp_scale=8)
write("hc_i32_scale8_smooth", (smooth_img * 1e5).astype(np.int32), compression_type="HCOMPRESS_1", hcomp_scale=8, hcomp_smooth=True)
write("hc_f32_q4", smooth_img.astype(np.float32), compression_type="HCOMPRESS_1", quantize_level=4, dither_seed=42)
write("hc_f32_q4_scale2_smooth", smooth_img.astype(np.float32), compression_type="HCOMPRESS_1", quantize_level=4, hcomp_scale=2, hcomp_smooth=True, dither_seed=42)
write("hc_f64_q8", smooth_img, compression_type="HCOMPRESS_1", quantize_level=8, dither_seed=9)
write("plio_i16", (smooth_img - 600).clip(0).astype(np.int16), compression_type="PLIO_1")
write("plio_i32_mask", ((x // 5 + y // 3) % 4 * 1000 + (x == y)).astype(np.int32), compression_type="PLIO_1")
write("plio_u8_tiles_16x8", ((x * y) % 200).astype(np.uint8), compression_type="PLIO_1", tile_shape=(8, 16))

# refimage pre6's two HCOMPRESS outputs.
for name in ["u16_hcompress", "f32_hcompress"]:
    src = os.path.expanduser(f"~/scratch/refimage-probe/fx/{name}.fits")
    dst = os.path.join(out, f"refimage_{name}.fits")
    shutil.copy(src, dst)
    with fits.open(dst) as f:
        np.asarray(f[-1].data, dtype=np.float64).astype("<f8").tofile(os.path.join(out, f"refimage_{name}.expected"))
    print("refimage", name)
