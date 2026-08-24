use std::{future::Future, time::Duration};

/// Result of running claimed work while the claim is renewed in the background.
#[derive(Debug, PartialEq, Eq)]
pub enum LeaseRun<T, E> {
    Completed(Result<T, E>),
    Lost,
    HeartbeatFailed(E),
}

/// Runs `work` while renewing its database lease at one third of the lease period.
///
/// Dropping the work future on `Lost`/`HeartbeatFailed` prevents an old owner from
/// continuing to stage or publish output. The final database mutation must still
/// fence on the unexpired claim token.
pub async fn run_with_heartbeat<T, E, Work, Heartbeat, HeartbeatFuture>(
    lease: Duration,
    work: Work,
    mut heartbeat: Heartbeat,
) -> LeaseRun<T, E>
where
    Work: Future<Output = Result<T, E>>,
    Heartbeat: FnMut() -> HeartbeatFuture,
    HeartbeatFuture: Future<Output = Result<bool, E>>,
{
    let period = lease
        .checked_div(3)
        .unwrap_or(Duration::from_millis(1))
        .max(Duration::from_millis(1));
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await;
    tokio::pin!(work);

    loop {
        tokio::select! {
            result = &mut work => return LeaseRun::Completed(result),
            _ = ticker.tick() => match heartbeat().await {
                Ok(true) => {}
                Ok(false) => return LeaseRun::Lost,
                Err(error) => return LeaseRun::HeartbeatFailed(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn renews_while_claimed_work_is_running() {
        let calls = Arc::new(AtomicUsize::new(0));
        let heartbeat_calls = calls.clone();
        let result = run_with_heartbeat(
            Duration::from_millis(30),
            async {
                tokio::time::sleep(Duration::from_millis(35)).await;
                Ok::<_, &'static str>("done")
            },
            move || {
                let heartbeat_calls = heartbeat_calls.clone();
                async move {
                    heartbeat_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(true)
                }
            },
        )
        .await;

        assert_eq!(result, LeaseRun::Completed(Ok("done")));
        assert!(calls.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn stops_work_as_soon_as_the_lease_is_lost() {
        let result = run_with_heartbeat(
            Duration::from_millis(15),
            async {
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok::<_, &'static str>(())
            },
            || async { Ok(false) },
        )
        .await;

        assert_eq!(result, LeaseRun::Lost);
    }
}
