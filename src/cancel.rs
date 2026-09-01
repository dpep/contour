//! One in-flight request's cancellation flag, carried ambiently.
//!
//! A stdio MCP server that blocks on the work it is doing cannot notice
//! anything — not `notifications/cancelled`, not the client hanging up. The
//! server therefore reads on one thread and works on others (DEC-031), and
//! this is how the reader reaches into work already running: it sets a flag,
//! and the loops that burn CPU look at it.
//!
//! **Why ambient rather than a parameter.** The alternative is a `&Cancel` on
//! every signature between the tool call and the loop — `search`, `similar`,
//! `dupes::find`, `dupes::find_near`, `near::pairs`, `vectors_for`,
//! `embed_all`. Six of those seven have a CLI caller with nothing to pass but
//! a token that is never set, and every function added later would have to
//! remember to thread it. That is a parameter that costs as much as it hides.
//!
//! **The one rule, and it is sharp.** The flag lives in a thread-local, and a
//! `rayon` worker is a different thread. A loop that hands work to rayon must
//! take [`current`] on its own thread and let the closure capture it — reading
//! it inside a parallel closure gets a fresh, never-cancelled token and the
//! run does not stop. [`crate::embed::embed_all`] is the worked example.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A flag one thread sets and another watches. Cloning shares the flag.
#[derive(Clone, Default, Debug)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// The one wording an abandoned request fails with, so a caller that sees
    /// it in a log or a tool result knows it was asked for.
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.cancelled(), "cancelled by the client");
        Ok(())
    }
}

thread_local! {
    /// Never cancelled where nothing set one — which is every CLI run, and is
    /// why no command outside the server has to know this module exists.
    static CURRENT: RefCell<Cancel> = RefCell::new(Cancel::new());
}

/// This thread's token.
pub fn current() -> Cancel {
    CURRENT.with(|slot| slot.borrow().clone())
}

/// Run `f` with `cancel` as this thread's token, restoring the previous one
/// however `f` leaves.
///
/// Restored through a guard rather than a trailing assignment: a panicking
/// tool call would otherwise leave a cancelled token behind on a pooled
/// worker, and every request that thread served afterwards would refuse before
/// doing any work — a failure that outlives its cause is the worst kind to
/// find in a log.
pub fn with<T>(cancel: &Cancel, f: impl FnOnce() -> T) -> T {
    struct Restore(Cancel);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|slot| *slot.borrow_mut() = self.0.clone());
        }
    }
    let _restore = Restore(current());
    CURRENT.with(|slot| *slot.borrow_mut() = cancel.clone());
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_with_no_token_is_never_cancelled() {
        assert!(!current().cancelled());
        assert!(current().check().is_ok());
    }

    #[test]
    fn the_token_is_restored_even_when_the_work_panics() {
        let cancel = Cancel::new();
        let panicked = std::panic::catch_unwind(|| {
            with(&cancel, || panic!("a tool call blew up"));
        });
        assert!(panicked.is_err());
        cancel.cancel();
        assert!(
            !current().cancelled(),
            "a pooled worker must come back clean"
        );
    }

    #[test]
    fn a_cancelled_token_is_seen_by_every_holder_of_it() {
        let cancel = Cancel::new();
        with(&cancel, || {
            assert!(!current().cancelled());
            cancel.cancel();
            assert!(current().cancelled());
            assert!(current().check().is_err());
        });
    }
}
