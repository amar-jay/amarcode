//! Cooperative cancellation for synchronous maintenance work and its lifetime.
use crate::{Error, Result};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::oneshot;

#[derive(Clone, Default)]
pub(crate) struct Shutdown(Arc<AtomicBool>);
impl Shutdown {
    pub fn cancel(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn check(&self) -> Result<()> {
        if self.requested() {
            Err(Error::msg("Daemon is shutting down"))
        } else {
            Ok(())
        }
    }
    pub(crate) async fn cancelled(&self) {
        while !self.requested() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    /// Called on synchronous installer threads (or inside block_in_place).
    /// Dropping an async HTTP future cancels stalled headers and body reads.
    pub fn wait<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        self.check()?;
        let cancellable = async {
            tokio::select! {
                biased;
                _ = self.cancelled() => Err(Error::msg("Daemon is shutting down")),
                result = future => result,
            }
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.block_on(cancellable)
        } else {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(cancellable)
        }
    }
}

struct Worker {
    name: &'static str,
    done: oneshot::Receiver<()>,
    thread: std::thread::JoinHandle<()>,
}

pub(crate) struct Background {
    pub shutdown: Shutdown,
    workers: Mutex<Vec<Worker>>,
}
impl Background {
    pub fn new() -> Self {
        Self {
            shutdown: Shutdown::default(),
            workers: Mutex::new(Vec::new()),
        }
    }
    pub fn spawn(&self, name: &'static str, job: impl FnOnce() + Send + 'static) {
        self.spawn_inner(name, job, false);
    }
    pub fn spawn_cleanup(&self, name: &'static str, job: impl FnOnce() + Send + 'static) {
        self.spawn_inner(name, job, true);
    }
    fn spawn_inner(&self, name: &'static str, job: impl FnOnce() + Send + 'static, cleanup: bool) {
        let mut workers = self.workers.lock().unwrap_or_else(|e| e.into_inner());
        if !cleanup && self.shutdown.requested() {
            return;
        }
        let shutdown = self.shutdown.clone();
        let (send, done) = oneshot::channel();
        // Dedicated threads cannot hold Tokio's runtime shutdown hostage if an
        // external service manager or library becomes unresponsive.
        match std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                if cleanup || !shutdown.requested() {
                    job();
                }
                let _ = send.send(());
            }) {
            Ok(thread) => workers.push(Worker { name, done, thread }),
            Err(error) => tracing::warn!(%name, %error, "could not start background worker"),
        }
    }
    pub async fn finish(&self) {
        self.finish_with_timeout(Duration::from_secs(5)).await;
    }
    async fn finish_with_timeout(&self, timeout: Duration) {
        self.shutdown.cancel();
        let workers = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|e| e.into_inner()));
        let deadline = tokio::time::Instant::now() + timeout;
        for worker in workers {
            match tokio::time::timeout_at(deadline, worker.done).await {
                Ok(_) => {
                    if worker.thread.join().is_err() {
                        tracing::warn!(name = worker.name, "background worker panicked");
                    }
                }
                Err(_) => tracing::warn!(
                    name = worker.name,
                    "background worker exceeded shutdown deadline"
                ),
            }
        }
    }
}
impl Drop for Background {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn finish_is_bounded_and_rejects_new_maintenance_jobs() {
        let background = Background::new();
        let (started, ready) = oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (finished, done) = oneshot::channel();
        background.spawn("stuck-external-call", move || {
            let _ = started.send(());
            let _ = wait.recv();
            let _ = finished.send(());
        });
        ready.await.unwrap();
        let now = std::time::Instant::now();
        background
            .finish_with_timeout(Duration::from_millis(50))
            .await;
        assert!(now.elapsed() < Duration::from_secs(1));
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        background.spawn("too-late", move || {
            flag.store(true, Ordering::SeqCst);
        });
        release.send(()).unwrap();
        done.await.unwrap();
        background.finish().await;
        assert!(!ran.load(Ordering::SeqCst));
    }
}
