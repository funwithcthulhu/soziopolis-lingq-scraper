use super::*;

impl SoziopolisLingqGui {
    pub(super) fn render_diagnostics_view(&mut self, ui: &mut egui::Ui) {
        let data_dir = app_paths::data_dir().ok();
        let log_path = app_paths::app_log_path().ok();
        let exe_path = std::env::current_exe().ok();

        ui.heading("Diagnostics");
        ui.add_space(8.0);
        framed_panel(ui, |ui| {
            let credential_status = credential_store::load_lingq_api_key()
                .ok()
                .flatten()
                .map(|_| "available")
                .unwrap_or("not found");
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                if let Some(path) = &data_dir {
                    ui.label(format!("Data: {}", path.display()));
                }
                ui.label(format!("Credential Manager: {}", credential_status));
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Open data folder").clicked() {
                    if let Some(path) = &data_dir {
                        if let Err(err) = open_path_in_explorer(path) {
                            self.set_notice(err, NoticeKind::Error);
                        }
                    }
                }
                if ui.button("Open log file").clicked() {
                    if let Some(path) = &log_path {
                        if let Err(err) = open_log_in_notepad(path) {
                            self.set_notice(err, NoticeKind::Error);
                        }
                    }
                }
                if ui.button("Copy recent log").clicked() {
                    match read_recent_log_excerpt(30) {
                        Ok(text) => {
                            ui.ctx().copy_text(text);
                            self.set_notice("Copied recent log lines.", NoticeKind::Success);
                        }
                        Err(err) => self.set_notice(err, NoticeKind::Error),
                    }
                }
                if ui.button("Create support bundle").clicked() {
                    match create_support_bundle(self) {
                        Ok(path) => {
                            if let Err(err) = open_path_in_explorer(&path) {
                                self.set_notice(
                                    format!(
                                        "Created support bundle at {}, but could not open it: {err}",
                                        path.display()
                                    ),
                                    NoticeKind::Info,
                                );
                            } else {
                                self.set_notice(
                                    format!("Created support bundle at {}.", path.display()),
                                    NoticeKind::Success,
                                );
                            }
                        }
                        Err(err) => self.set_notice(err, NoticeKind::Error),
                    }
                }
            });
            if let Some(path) = &exe_path {
                ui.small(
                    RichText::new(format!("Executable: {}", path.display()))
                        .color(Color32::from_gray(165)),
                );
            }
            if let Some(path) = &log_path {
                ui.small(
                    RichText::new(format!("Log: {}", path.display()))
                        .color(Color32::from_gray(165)),
                );
            }
        });

        ui.add_space(10.0);
        framed_panel(ui, |ui| {
            ui.label(RichText::new("Jobs").strong());
            ui.add_space(6.0);
            if let Some(active_job) = &self.active_job {
                ui.label(RichText::new(&active_job.label).strong());
                let fraction = if active_job.total == 0 {
                    0.0
                } else {
                    active_job.processed as f32 / active_job.total as f32
                };
                ui.add(ProgressBar::new(fraction.clamp(0.0, 1.0)).text(format!(
                    "{} / {} complete",
                    active_job.processed, active_job.total
                )));
                ui.small(format!(
                    "Success {}, failed {}, current {}",
                    active_job.succeeded,
                    active_job.failed,
                    if active_job.current_item.is_empty() {
                        "waiting...".to_owned()
                    } else {
                        truncate_for_ui(&active_job.current_item, 80)
                    }
                ));
                if ui.button("Cancel current job").clicked() {
                    self.cancel_active_job();
                }
            } else {
                ui.small(
                    RichText::new("No running import or upload job.")
                        .color(Color32::from_gray(150)),
                );
            }

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Queue: {}",
                    if self.queue_paused {
                        "Paused"
                    } else {
                        "Running"
                    }
                ));
                ui.label(format!("Queued jobs: {}", self.queued_jobs.len()));
                if ui
                    .add_enabled(!self.queue_paused, egui::Button::new("Pause queue"))
                    .clicked()
                {
                    self.pause_queue();
                }
                if ui
                    .add_enabled(self.queue_paused, egui::Button::new("Resume queue"))
                    .clicked()
                {
                    self.resume_queue();
                }
                if ui
                    .add_enabled(
                        self.active_job.is_none()
                            && self
                                .queued_jobs
                                .iter()
                                .any(|job| matches!(job.request, QueuedJobRequest::Upload { .. })),
                        egui::Button::new("Run queued upload now"),
                    )
                    .clicked()
                {
                    self.run_queued_upload_now();
                }
                if ui
                    .add_enabled(
                        !self.queued_jobs.is_empty(),
                        egui::Button::new("Clear queued jobs"),
                    )
                    .clicked()
                {
                    self.queued_jobs.clear();
                    self.persist_queue_state();
                    self.set_notice("Cleared queued jobs.", NoticeKind::Info);
                }
                if ui
                    .add_enabled(
                        !self.failed_fetches.is_empty(),
                        egui::Button::new("Retry failed imports"),
                    )
                    .clicked()
                {
                    self.retry_failed_fetches();
                }
                if ui
                    .add_enabled(
                        !self.last_failed_uploads.is_empty(),
                        egui::Button::new("Retry failed uploads"),
                    )
                    .clicked()
                {
                    self.retry_failed_uploads();
                }
            });

            if !self.queued_jobs.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new("Queue").strong());
                for job in self.queued_jobs.iter().take(6) {
                    ui.small(format!(
                        "#{} {} ({}){}",
                        job.id,
                        job.label,
                        job.kind.label(),
                        if self.queue_paused
                            && matches!(job.request, QueuedJobRequest::Upload { .. })
                        {
                            " [waiting]"
                        } else {
                            ""
                        }
                    ));
                }
            }

            if !self.completed_jobs.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Recent jobs").strong());
                for job in self.completed_jobs.iter().take(8) {
                    ui.small(format!(
                        "#{} {} ({}) [{}] {}{}",
                        job.id,
                        job.label,
                        job.kind.label(),
                        if job.success { "ok" } else { "issue" },
                        job.summary,
                        if job.recorded_at.is_empty() {
                            String::new()
                        } else {
                            format!(" @ {}", job.recorded_at)
                        }
                    ));
                }
            }
        });

        ui.add_space(10.0);
        framed_panel(ui, |ui| {
            ui.label(RichText::new("Recent log excerpt").strong());
            ui.add_space(6.0);
            match read_recent_log_excerpt(18) {
                Ok(text) => {
                    ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.code(text);
                    });
                }
                Err(err) => {
                    ui.small(RichText::new(err).color(Color32::from_rgb(238, 100, 100)));
                }
            }
        });
    }
}
