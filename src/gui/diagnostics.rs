use super::*;

impl SoziopolisLingqGui {
    pub(super) fn render_diagnostics_view(&mut self, ui: &mut egui::Ui) {
        let data_dir = app_paths::data_dir().ok();
        let log_path = app_paths::app_log_path().ok();
        let exe_path = std::env::current_exe().ok();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
                        let perf = crate::perf::snapshot();
                        ui.label(format!(
                            "Browse cache H/M: {}/{}",
                            perf.browse_cache_hits, perf.browse_cache_misses
                        ));
                        ui.label(format!(
                            "Summary cache H/M: {}/{}",
                            perf.browse_summary_cache_hits, perf.browse_summary_cache_misses
                        ));
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
                        if ui.button("Clear browse cache").clicked() {
                            match app_ops::clear_browse_cache() {
                                Ok(removed) => {
                                    self.set_notice(
                                        format!("Cleared {} cached browse file(s).", removed),
                                        NoticeKind::Success,
                                    );
                                }
                                Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                            }
                        }
                        if ui.button("Compact local data").clicked() {
                            match self
                                .app_context()
                                .map_err(anyhow::Error::msg)
                                .and_then(|ctx| app_ops::compact_local_data(&ctx))
                            {
                                Ok(()) => {
                                    self.set_notice(
                                        "Compacted the local database and trimmed the SQLite WAL.",
                                        NoticeKind::Success,
                                    );
                                }
                                Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                            }
                        }
                        if ui.button("Rebuild search index").clicked() {
                            match self
                                .app_context()
                                .map_err(anyhow::Error::msg)
                                .and_then(|ctx| app_ops::rebuild_search_index(&ctx))
                            {
                                Ok(()) => {
                                    self.set_notice(
                                        "Rebuilt the local search index.",
                                        NoticeKind::Success,
                                    )
                                }
                                Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
                            }
                        }
                        if ui.button("Verify database").clicked() {
                            match self
                                .app_context()
                                .map_err(anyhow::Error::msg)
                                .and_then(|ctx| app_ops::verify_database(&ctx))
                            {
                                Ok(result) => {
                                    self.set_notice(
                                        format!("SQLite integrity check: {result}"),
                                        NoticeKind::Info,
                                    )
                                }
                                Err(err) => self.set_notice(err.to_string(), NoticeKind::Error),
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
                self.render_jobs_diagnostics_panel(ui);

                ui.add_space(10.0);
                self.render_failure_diagnostics_panel(ui);

                ui.add_space(10.0);
                self.render_task_failure_diagnostics_panel(ui);

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
                ui.add_space(12.0);
            });
    }

    fn render_jobs_diagnostics_panel(&mut self, ui: &mut egui::Ui) {
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
                ui.label(format!("Stored history: {}", self.completed_jobs.len()));
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
            });

            if !self.queued_jobs.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Queue").strong());
                for job in self.queued_jobs.iter().take(10) {
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

            ui.add_space(10.0);
            ui.label(RichText::new("Job history").strong());
            ui.small(
                RichText::new("Select a recent job to inspect its summary and timestamp.")
                    .color(Color32::from_gray(160)),
            );
            ui.add_space(6.0);

            if self.completed_jobs.is_empty() {
                ui.small(
                    RichText::new("No completed jobs have been recorded yet.")
                        .color(Color32::from_gray(150)),
                );
                return;
            }

            if self
                .diagnostics_selected_job_id
                .is_none_or(|job_id| !self.completed_jobs.iter().any(|job| job.id == job_id))
            {
                self.diagnostics_selected_job_id = self.completed_jobs.front().map(|job| job.id);
            }

            ui.columns(2, |columns| {
                columns[0].set_min_width(320.0);
                ScrollArea::vertical()
                    .max_height(280.0)
                    .show(&mut columns[0], |ui| {
                        for job in self.completed_jobs.iter().take(25) {
                            let selected = self.diagnostics_selected_job_id == Some(job.id);
                            let label = format!(
                                "#{} {} [{}]",
                                job.id,
                                truncate_for_ui(&job.label, 34),
                                if job.success { "ok" } else { "issue" }
                            );
                            if ui.selectable_label(selected, label).clicked() {
                                self.diagnostics_selected_job_id = Some(job.id);
                            }
                            ui.small(
                                RichText::new(format_job_timestamp(&job.recorded_at))
                                    .color(Color32::from_gray(150)),
                            );
                            ui.add_space(4.0);
                        }
                    });

                if let Some(selected_job) = self
                    .diagnostics_selected_job_id
                    .and_then(|job_id| self.completed_jobs.iter().find(|job| job.id == job_id))
                {
                    framed_panel(&mut columns[1], |ui| {
                        ui.label(RichText::new(&selected_job.label).strong());
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            tag(ui, selected_job.kind.label());
                            if selected_job.success {
                                success_tag(ui, "Successful");
                            } else {
                                tag(ui, "Finished with issues");
                            }
                            tag(ui, &format!("Job #{}", selected_job.id));
                        });
                        ui.add_space(6.0);
                        ui.small(format!(
                            "Completed: {}",
                            format_job_timestamp(&selected_job.recorded_at)
                        ));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Summary").strong());
                        ui.add_space(4.0);
                        ui.label(&selected_job.summary);
                    });
                }
            });
        });
    }

    fn render_failure_diagnostics_panel(&mut self, ui: &mut egui::Ui) {
        framed_panel(ui, |ui| {
            ui.label(RichText::new("Retained failures").strong());
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
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
            ui.add_space(8.0);
            ui.columns(2, |columns| {
                columns[0].label(RichText::new("Import failures").strong());
                columns[0].small(format!("{} retained item(s)", self.failed_fetches.len()));
                ScrollArea::vertical()
                    .max_height(220.0)
                    .show(&mut columns[0], |ui| {
                        if self.failed_fetches.is_empty() {
                            ui.small(
                                RichText::new("No retained import failures.")
                                    .color(Color32::from_gray(150)),
                            );
                        } else {
                            for item in &self.failed_fetches {
                                ui.small(RichText::new(format!(
                                    "[{}] {}",
                                    item.category,
                                    if item.title.is_empty() {
                                        &item.url
                                    } else {
                                        &item.title
                                    }
                                )));
                                ui.small(
                                    RichText::new(truncate_for_ui(&item.message, 140))
                                        .color(Color32::from_gray(155)),
                                );
                                ui.add_space(6.0);
                            }
                        }
                    });

                columns[1].label(RichText::new("Upload failures").strong());
                columns[1].small(format!(
                    "{} retained item(s)",
                    self.last_failed_uploads.len()
                ));
                ScrollArea::vertical()
                    .max_height(220.0)
                    .show(&mut columns[1], |ui| {
                        if self.last_failed_uploads.is_empty() {
                            ui.small(
                                RichText::new("No retained upload failures.")
                                    .color(Color32::from_gray(150)),
                            );
                        } else {
                            for item in &self.last_failed_uploads {
                                ui.small(RichText::new(format!(
                                    "#{} {}",
                                    item.article_id,
                                    if item.title.is_empty() {
                                        "Upload item"
                                    } else {
                                        &item.title
                                    }
                                )));
                                ui.small(
                                    RichText::new(truncate_for_ui(&item.message, 140))
                                        .color(Color32::from_gray(155)),
                                );
                                ui.add_space(6.0);
                            }
                        }
                    });
            });
        });
    }

    fn render_task_failure_diagnostics_panel(&mut self, ui: &mut egui::Ui) {
        framed_panel(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Recent task failures").strong());
                ui.small(format!(
                    "{} retained item(s)",
                    self.recent_task_failures.len()
                ));
                if ui
                    .add_enabled(
                        !self.recent_task_failures.is_empty(),
                        egui::Button::new("Clear task failures"),
                    )
                    .clicked()
                {
                    self.recent_task_failures.clear();
                    self.set_notice("Cleared retained task failures.", NoticeKind::Info);
                }
            });
            ui.add_space(6.0);
            ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                if self.recent_task_failures.is_empty() {
                    ui.small(
                        RichText::new("No retained task failures.").color(Color32::from_gray(150)),
                    );
                } else {
                    for failure in &self.recent_task_failures {
                        ui.label(
                            RichText::new(format!(
                                "[{}] {}",
                                failure.kind.label(),
                                failure.operation
                            ))
                            .strong(),
                        );
                        ui.small(
                            RichText::new(format_job_timestamp(&failure.recorded_at))
                                .color(Color32::from_gray(150)),
                        );
                        ui.small(&failure.message);
                        if let Some(details) = &failure.details {
                            ui.small(
                                RichText::new(truncate_for_ui(details, 180))
                                    .color(Color32::from_gray(155)),
                            );
                        }
                        ui.add_space(8.0);
                    }
                }
            });
        });
    }
}
