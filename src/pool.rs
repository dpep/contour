//! The one place work is handed to a thread pool.
//!
//! rayon's global pool is lazy, but it is created by *whatever touches it
//! first* — and what touched it first here was a `par_iter` over an **empty**
//! list. Every command opens the index, and opening it refreshes it
//! ([`crate::index::index`]); on a checkout where nothing moved there is
//! nothing to parse, and that empty parallel map still built a thread per
//! core, parked them all and tore them down. A field report on a monorepo
//! measured exactly that: a full CPU-sized pool idle in a condition-variable
//! wait for the whole of a trivial scoped query.
//!
//! So the rule is one sentence, and it is not a threshold: **never hand rayon
//! nothing.** Anything else goes to the same one global pool it always did,
//! which is what DEC-031's worker bound rests on.
//!
//! **Why not a size threshold, which is the obvious next step.** Measured
//! before being written: building the pool costs about 0.4 ms on 8 cores (50
//! warm scoped queries, 6.17 s against 6.15 s, and 194 involuntary context
//! switches against 114), while one blob parse costs about 0.3 ms of CPU
//! (17,639 blobs, 5.26 s of user time). A pool that fixed therefore pays for
//! itself at about **two** items, so any threshold worth naming would make
//! real work serial to save a fraction of a millisecond. The one workload with
//! a genuinely large crossover is embedding — each worker loads its own ONNX
//! session — and [`crate::embed::embed_all`] already carries that measurement
//! and its own decision. One rule here, not a number to keep calibrated.
//!
//! Order-preserving, because `embed_all` needs its results index-aligned with
//! its input and a second contract would be one more thing to remember.

use rayon::prelude::*;

/// Map `f` over `items` in parallel, without building a pool for nothing.
pub(crate) fn map<T, R>(items: &[T], f: impl Fn(&T) -> R + Send + Sync) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    if items.is_empty() {
        return Vec::new();
    }
    items.par_iter().map(f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_maps_in_order_and_answers_nothing_for_nothing() {
        let items: Vec<usize> = (0..200).collect();
        let expect: Vec<usize> = items.iter().map(|i| i * 3).collect();
        assert_eq!(map(&items, |i| i * 3), expect);
        assert!(map::<usize, usize>(&[], |i| *i).is_empty());
    }
}
