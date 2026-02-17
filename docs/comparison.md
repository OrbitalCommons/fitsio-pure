# Comparison with other Rust FITS libraries

| | **fitsio-pure** | **fitsio** | **fitsrs** |
|---|---|---|---|
| **Pure Rust** | ✅ | ❌ | ✅ |
| **External deps** | None | cfitsio + C toolchain | None |
| **`wasm32` / `no_std`** | ✅ / ✅ | ❌ / ❌ | ❌ / ❌ |
| **License** | Apache-2.0 | MIT/Apache-2.0 | MIT/Apache-2.0 |
| | | | |
| **Read images** | ✅ All BITPIX | ✅ All BITPIX | ✅ All BITPIX |
| **Write images** | ✅ All BITPIX | ✅ All BITPIX | ✅ All BITPIX |
| **Binary tables** | ✅ Read + write | ✅ Read + write | ⚠️ Read only |
| **ASCII tables** | ✅ Read + write | ✅ Read + write | ⚠️ Raw bytes |
| **Random groups** | ⚠️ Read | ✅ Read + write | ❌ |
| **Tile compression** | ✅ RICE_1/GZIP_1 read | ✅ Transparent | ⚠️ GZIP/RICE |
| **Variable-length arrays** | ❌ | ✅ | ❌ |
| **BSCALE/BZERO** | ⚠️ Read | ✅ Read + write | ❌ |
| **Header keywords** | ✅ Read + write | ✅ Read + write | ⚠️ Read only |
| **Async I/O** | ❌ | ❌ | ✅ |
| **ndarray** | ✅ | ✅ | ❌ |
| **cfitsio compat API** | ✅ | — | ❌ |
| | | | |
| **Image read speed** | 0.3–1x | 1x | — |
| **Image write speed** | 0.3–1x | 1x | — |
| **Column read speed** | 0.3–1.5x | 1x | — |
| **Column write speed** | 0.1–1x | 1x | — |
| | | | |
| **crates.io downloads** | 66 | 112K | 21K |
| **Repository** | [OrbitalCommons](https://github.com/OrbitalCommons/fitsio-pure) | [simonrw](https://github.com/simonrw/rust-fitsio) | [cds-astro](https://github.com/cds-astro/fitsrs) |

Speeds are relative to fitsio/cfitsio (1x = baseline). Ranges span small to large data sizes — fitsio-pure matches cfitsio on small arrays and is ~3x slower on large images. Column writes at scale are the widest gap (~10x at 1M rows). See [benchmarks.md](benchmarks.md) for full numbers.

## Links

- [fitsio on crates.io](https://crates.io/crates/fitsio)
- [fitsrs on crates.io](https://crates.io/crates/fitsrs)
- [fitsio-pure on crates.io](https://crates.io/crates/fitsio-pure)
