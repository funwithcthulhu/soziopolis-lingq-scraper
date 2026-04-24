use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AppTaskKind {
    Browse,
    Preview,
    Refresh,
    Lingq,
    Import,
    Upload,
}

impl AppTaskKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Browse => "browse",
            Self::Preview => "preview",
            Self::Refresh => "refresh",
            Self::Lingq => "lingq",
            Self::Import => "import",
            Self::Upload => "upload",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct AppTaskHandle {
    cancel_flag: Arc<AtomicBool>,
}

impl AppTaskHandle {
    pub(super) fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub(super) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel_flag.clone()
    }
}

#[derive(Clone)]
pub(super) struct AppTaskRuntime {
    tx: Sender<AppEvent>,
    next_task_id: Arc<std::sync::atomic::AtomicU64>,
}

impl AppTaskRuntime {
    pub(super) fn new(tx: Sender<AppEvent>) -> Self {
        Self {
            tx,
            next_task_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    pub(super) fn spawn(
        &self,
        kind: AppTaskKind,
        task_label: impl Into<String>,
        run: impl FnOnce(AppTaskContext) -> AppEvent + Send + 'static,
        on_panic: impl FnOnce(AppError) -> AppEvent + Send + 'static,
    ) -> AppTaskHandle {
        let task_label = task_label.into();
        let task_id = self
            .next_task_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let handle = AppTaskHandle {
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        let worker_handle = handle.clone();
        let tx = self.tx.clone();

        logging::info(format!(
            "starting {} background task #{}: {}",
            kind.label(),
            task_id,
            task_label
        ));

        std::thread::spawn(move || {
            let context = AppTaskContext {
                tx: tx.clone(),
                handle: worker_handle.clone(),
            };
            let event = match panic::catch_unwind(AssertUnwindSafe(|| run(context))) {
                Ok(event) => event,
                Err(payload) => {
                    let message = format!(
                        "{} task #{} ({}) hit an internal error: {}",
                        kind.label(),
                        task_id,
                        task_label,
                        panic_payload_message(payload.as_ref())
                    );
                    logging::error(&message);
                    on_panic(AppError::internal_task(kind.label(), &task_label, message))
                }
            };
            let _ = tx.send(event);
        });

        handle
    }
}

#[derive(Clone)]
pub(super) struct AppTaskContext {
    tx: Sender<AppEvent>,
    handle: AppTaskHandle,
}

impl AppTaskContext {
    pub(super) fn emit(&self, event: AppEvent) {
        let _ = self.tx.send(event);
    }

    pub(super) fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.handle.cancel_flag()
    }
}
