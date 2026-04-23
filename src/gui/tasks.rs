use super::*;

pub(super) fn spawn_app_event_task(
    tx: Sender<AppEvent>,
    task_label: &'static str,
    run: impl FnOnce(Sender<AppEvent>) -> AppEvent + Send + 'static,
    on_panic: impl FnOnce(String) -> AppEvent + Send + 'static,
) {
    logging::info(format!("starting background task: {task_label}"));
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let event = match panic::catch_unwind(AssertUnwindSafe(|| run(progress_tx))) {
            Ok(event) => event,
            Err(payload) => {
                let message = format!(
                    "{task_label} hit an internal error: {}",
                    panic_payload_message(payload.as_ref())
                );
                logging::error(&message);
                on_panic(message)
            }
        };
        let _ = tx.send(event);
    });
}
