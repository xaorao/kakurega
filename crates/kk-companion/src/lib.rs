pub mod app;
pub mod security;

use app::{router, AppState, Extension, NoExtension};
use security::Security;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;

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
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ui_dir: None,
            security: Arc::new(Security::new()),
            extension: Arc::new(NoExtension),
        }
    }
}

pub fn start(options: Options) -> std::io::Result<Companion> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let state = AppState {
        sec: options.security.clone(),
        addr,
        ui_dir: options.ui_dir,
        extra: options.extension,
    };
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
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
    });
    Ok(Companion {
        addr,
        security: options.security,
        shutdown: Some(tx),
        thread: Some(thread),
    })
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
