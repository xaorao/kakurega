pub mod app;
pub mod security;

use app::{router, AppState, Extension, NoExtension};
use security::Security;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The most connections served at once. Each one holds a descriptor, and a program started from the desktop often has only
/// 256; a connection left open and idle by any program on the computer used to fill them and the service stopped answering.
pub const MAX_CONNECTIONS: usize = 128;
/// How long a connection may take to send the head of a request, or sit idle between requests, before it is closed. A real
/// client sends the head at once.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Companion {
    pub addr: SocketAddr,
    pub security: Arc<Security>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub struct Options {
    pub ui_dir: Option<PathBuf>,
    pub security: Arc<Security>,
    pub extension: Arc<dyn Extension>,
    /// When set, the app answers only requests that carry it in `x-kk-shell`: the desktop shell adds it to its own window's
    /// requests, so no other program on the computer can even load the page. At least 32 characters.
    pub shell_secret: Option<String>,
    /// The port to listen on when it is free (the one this data folder's service had last time), so a browser tab, the
    /// extension and a bookmark still reach the service after it starts again. Taken by another program, any free port
    /// is used.
    pub preferred_port: Option<u16>,
    /// The most connections served at once; more wait to be accepted.
    pub max_connections: usize,
    /// How long a connection may take to send a request head or stay idle before it is closed.
    pub header_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            preferred_port: None,
            ui_dir: None,
            security: Arc::new(Security::new()),
            extension: Arc::new(NoExtension),
            shell_secret: None,
            max_connections: MAX_CONNECTIONS,
            header_timeout: HEADER_TIMEOUT,
        }
    }
}

pub fn start(options: Options) -> std::io::Result<Companion> {
    if options.shell_secret.as_ref().is_some_and(|s| s.len() < 32) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the shell secret must be at least 32 characters",
        ));
    }
    let listener = match options
        .preferred_port
        .filter(|p| *p >= 1024)
        .and_then(|p| TcpListener::bind(("127.0.0.1", p)).ok())
    {
        Some(l) => l,
        None => TcpListener::bind("127.0.0.1:0")?,
    };
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let state = AppState {
        sec: options.security.clone(),
        addr,
        ui_dir: options.ui_dir,
        extra: options.extension,
        shell_secret: options.shell_secret,
    };
    let (max_connections, header_timeout) =
        (options.max_connections.max(1), options.header_timeout);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            let app = router(state);
            serve(listener, app, rx, max_connections, header_timeout).await;
        });
    });
    Ok(Companion {
        addr,
        security: options.security,
        shutdown: Some(tx),
        thread: Some(thread),
    })
}

/// Serves connections until `shutdown` fires. At most `max_connections` are open at once (the rest wait in the operating
/// system's queue, which costs no descriptor), a connection that does not send a complete request head within
/// `header_timeout` is closed, and each connection serves one request and is then closed, so no client holds an idle one. On shutdown each connection finishes the
/// request it is serving; those still open after five seconds are dropped.
async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
    max_connections: usize,
    header_timeout: Duration,
) {
    use hyper_util::rt::{TokioIo, TokioTimer};
    use hyper_util::service::TowerToHyperService;
    let slots = Arc::new(tokio::sync::Semaphore::new(max_connections));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    loop {
        let permit = tokio::select! {
            p = slots.clone().acquire_owned() => p.expect("the semaphore is never closed"),
            _ = &mut shutdown => break,
        };
        let stream = tokio::select! {
            r = listener.accept() => match r {
                Ok((stream, _)) => stream,
                Err(_) => {
                    // out of descriptors or a connection that went away before it was accepted: wait a moment
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
            _ = &mut shutdown => break,
        };
        let service = TowerToHyperService::new(app.clone());
        let mut stop = stop_rx.clone();
        tokio::spawn(async move {
            let conn = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                // one request per connection: a client that kept a connection could send its next request down one the
                // server has just closed for being idle
                .keep_alive(false)
                .header_read_timeout(header_timeout)
                .serve_connection(TokioIo::new(stream), service);
            tokio::pin!(conn);
            tokio::select! {
                _ = conn.as_mut() => {}
                _ = async { let _ = stop.wait_for(|stopped| *stopped).await; } => {
                    conn.as_mut().graceful_shutdown();
                    let _ = conn.as_mut().await;
                }
            }
            drop(permit);
        });
    }
    let _ = stop_tx.send(true);
    let _ = tokio::time::timeout(
        Duration::from_secs(5),
        slots.acquire_many(u32::try_from(max_connections).unwrap_or(u32::MAX)),
    )
    .await;
}

impl Drop for Companion {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
