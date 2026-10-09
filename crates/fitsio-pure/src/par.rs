//! Chunked loops that run on all cores with the `parallel` feature and
//! sequentially without it. The work done per chunk is the same either way,
//! so results are identical.

/// Pixels per chunk for whole-image conversions: large enough that a chunk
/// outweighs the cost of handing it to a thread.
pub(crate) const PIXEL_CHUNK: usize = 1 << 16;

/// Call `f(start, chunk)` for consecutive `len`-sized chunks of `out`, where
/// `start` is the chunk's offset in `out`.
pub(crate) fn chunks_mut<T, F>(out: &mut [T], len: usize, f: F)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync + Send,
{
    let len = len.max(1);
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        out.par_chunks_mut(len)
            .enumerate()
            .for_each(|(i, chunk)| f(i * len, chunk));
    }
    #[cfg(not(feature = "parallel"))]
    for (i, chunk) in out.chunks_mut(len).enumerate() {
        f(i * len, chunk);
    }
}

/// [`chunks_mut`] for a fallible `f`; the first error (in chunk order
/// without the feature, any one with it) is returned.
pub(crate) fn try_chunks_mut<T, E, F>(out: &mut [T], len: usize, f: F) -> Result<(), E>
where
    T: Send,
    E: Send,
    F: Fn(usize, &mut [T]) -> Result<(), E> + Sync + Send,
{
    let len = len.max(1);
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        out.par_chunks_mut(len)
            .enumerate()
            .try_for_each(|(i, chunk)| f(i * len, chunk))
    }
    #[cfg(not(feature = "parallel"))]
    {
        for (i, chunk) in out.chunks_mut(len).enumerate() {
            f(i * len, chunk)?;
        }
        Ok(())
    }
}

/// Fill `out` with `f(input element)`, element by element, in chunks.
pub(crate) fn map_into<S, T, F>(input: &[S], out: &mut [T], f: F)
where
    S: Sync,
    T: Send,
    F: Fn(&S) -> T + Sync + Send,
{
    chunks_mut(out, PIXEL_CHUNK, |start, chunk| {
        for (o, i) in chunk.iter_mut().zip(&input[start..]) {
            *o = f(i);
        }
    });
}
