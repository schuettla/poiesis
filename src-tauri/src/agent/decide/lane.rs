//! `SEE-1`: hot-path decisions go before check asks.
//!
//! The check of an answer asks about every sentence, so up to 40 asks can be waiting
//! when the person sends the next message. A hot-path ask (the tool menu, the
//! handover match, the tool-call gate: anything with `needs_fast`) must never queue
//! behind them. Two rules do it:
//!
//! - **Check asks go one at a time**, in the order they came. They hold [`Check`]
//!   for the whole request.
//! - **A check ask does not start while a hot ask is in flight or waiting.** A hot
//!   ask holds a [`Hot`] guard from the moment it is made until it has its answer
//!   (or has given up), and every check ask waits for the guard count to reach zero
//!   before its request goes out.
//!
//! So a hot ask overtakes everything that is queued, and costs at most the one
//! check request already on its way. A hot ask never waits for the check lock.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use tokio::sync::{Mutex, MutexGuard, Notify};

struct Lane {
    hot: AtomicUsize,
    quiet: Notify,
    checks: Mutex<()>,
}

fn lane() -> &'static Lane {
    static LANE: OnceLock<Lane> = OnceLock::new();
    LANE.get_or_init(|| Lane { hot: AtomicUsize::new(0), quiet: Notify::new(), checks: Mutex::new(()) })
}

/// A hot-path ask is in progress. Dropping it says it is over.
pub struct Hot(());

pub fn hot() -> Hot {
    lane().hot.fetch_add(1, Ordering::SeqCst);
    Hot(())
}

impl Drop for Hot {
    fn drop(&mut self) {
        let l = lane();
        if l.hot.fetch_sub(1, Ordering::SeqCst) == 1 {
            l.quiet.notify_waiters();
        }
    }
}

/// How many hot asks are in progress now.
pub fn hot_now() -> usize {
    lane().hot.load(Ordering::SeqCst)
}

/// A check ask's turn: held while its request is on its way.
pub struct Check(#[allow(dead_code)] MutexGuard<'static, ()>);

/// Wait for this check ask's turn: the earlier check asks first, and no hot ask in
/// flight or waiting.
pub async fn check_turn() -> Check {
    let l = lane();
    let guard = l.checks.lock().await;
    loop {
        // Registered before the count is read, so a hot ask that ends in between is not missed.
        let quiet = l.quiet.notified();
        if l.hot.load(Ordering::SeqCst) == 0 {
            break;
        }
        quiet.await;
    }
    Check(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex as AsyncMutex;

    /// The lane is one per app, so these tests run one after the other.
    static ONE_AT_A_TIME: AsyncMutex<()> = AsyncMutex::const_new(());

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(40)).await;
    }

    /// `SEE-T-4`: a hot ask made while check asks are queued is not held up by them,
    /// and they do not go out until it is over. Then they go in the order they came.
    #[tokio::test]
    async fn a_hot_ask_overtakes_queued_check_asks() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));

        // Three check asks are queued; none has gone out yet because a hot ask is in flight.
        let first = hot();
        let mut tasks = Vec::new();
        for name in ["check 1", "check 2", "check 3"] {
            let order = order.clone();
            tasks.push(tokio::spawn(async move {
                let _turn = check_turn().await;
                order.lock().unwrap().push(name);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }));
            settle().await;
        }
        assert!(order.lock().unwrap().is_empty(), "checks wait for the hot ask");

        // Another hot ask arrives while they wait and gets in at once: it needs no lock.
        let second = hot();
        assert_eq!(hot_now(), 2);
        order.lock().unwrap().push("hot 2");
        drop(first);
        settle().await;
        assert_eq!(*order.lock().unwrap(), ["hot 2"], "one hot ask is still going, so the checks still wait");
        drop(second);
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(*order.lock().unwrap(), ["hot 2", "check 1", "check 2", "check 3"]);
        assert_eq!(hot_now(), 0);
    }

    /// With no hot ask there is nothing to wait for, and check asks go one at a time.
    #[tokio::test]
    async fn check_asks_go_one_at_a_time_when_nothing_is_hot() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let running = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..4 {
            let (running, most) = (running.clone(), most.clone());
            tasks.push(tokio::spawn(async move {
                let _turn = check_turn().await;
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                running.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1);
    }

    /// A hot ask that gives up (is dropped) lets the checks go.
    #[tokio::test]
    async fn a_hot_ask_that_is_dropped_lets_the_checks_go() {
        let _serial = ONE_AT_A_TIME.lock().await;
        let h = hot();
        let waiting = tokio::spawn(async { check_turn().await });
        settle().await;
        assert!(!waiting.is_finished());
        drop(h);
        let turn = tokio::time::timeout(Duration::from_secs(2), waiting).await.expect("released").unwrap();
        drop(turn);
    }
}
