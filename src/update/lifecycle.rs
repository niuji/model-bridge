use std::{future::Future, time::Duration};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub async fn drain<A, B, S>(
    proxy: A,
    admin: B,
    signal: S,
    stop: CancellationToken,
    writes: TaskTracker,
    limit: Duration,
) -> anyhow::Result<()>
where
    A: Future<Output = std::io::Result<()>>,
    B: Future<Output = std::io::Result<()>>,
    S: Future<Output = ()>,
{
    tokio::pin!(proxy, admin, signal);
    let (proxy_done, admin_done, first_error) = tokio::select! {
        result=&mut proxy=>(true,false,result.err()),
        result=&mut admin=>(false,true,result.err()),
        _=&mut signal=>(false,false,None),
    };
    stop.cancel();
    tokio::time::timeout(limit, async {
        // Finish both listeners before closing the tracker: handlers may still spawn usage writes.
        let (p, a) = tokio::join!(
            async {
                if !proxy_done {
                    proxy.await
                } else {
                    Ok(())
                }
            },
            async {
                if !admin_done {
                    admin.await
                } else {
                    Ok(())
                }
            }
        );
        writes.close();
        writes.wait().await;
        p?;
        a?;
        if let Some(e) = first_error {
            return Err(e.into());
        }
        Ok(())
    })
    .await
    .map_err(|_| anyhow::anyhow!("request drain exceeded deadline"))?
}
pub fn spawn_background<F>(
    tasks: &TaskTracker,
    updates: std::sync::Arc<super::Manager>,
    stop: CancellationToken,
    future: F,
) where
    F: Future<Output = ()> + Send + 'static,
{
    tasks.spawn(async move {
        tokio::select! {
            _=stop.cancelled()=>{},
            _=async {updates.wait_active().await;future.await;}=>{},
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn admin_completion_must_not_drop_proxy_or_pending_writes() {
        let (sent, mut received) = tokio::sync::oneshot::channel();
        let writes = TaskTracker::new();
        writes.spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            sent.send(()).unwrap();
        });
        let proxy = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(())
        };
        drain(
            proxy,
            async { Ok(()) },
            async {},
            CancellationToken::new(),
            writes,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(received.try_recv().is_ok());
    }
    #[tokio::test]
    async fn drain_timeout_is_an_error_not_a_clean_shutdown() {
        assert!(drain(
            std::future::pending(),
            async { Ok(()) },
            async {},
            CancellationToken::new(),
            TaskTracker::new(),
            Duration::from_millis(10)
        )
        .await
        .is_err());
    }
}
