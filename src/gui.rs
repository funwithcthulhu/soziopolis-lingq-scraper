use crate::{
    app_paths, commands,
    context::AppContext,
    credential_store,
    database::Database,
    database::{LibraryStats, SectionCount, StoredArticle},
    domain::ArticleListItem,
    jobs::{
        CompletedJob, FailedFetchItem, ImportProgress, JobKind, QueueSnapshot, QueuedJob,
        QueuedJobRequest, UploadFailure, UploadProgress, UploadSuccess,
    },
    lingq::Collection,
    logging,
    repositories::{ArticleRepository, JobRepository},
    services::{
        BrowseResponse, BrowseService, BrowseSessionState, ContentRefreshResult, LingqService,
    },
    settings::SettingsStore,
    soziopolis::{Article, ArticleSummary, DiscoveryReport, SECTIONS},
    topics::generated_topic_from_fields,
};
use chrono::NaiveDate;
use eframe::egui::{
    self, Align, Color32, Context, Frame, Layout, Margin, Panel, ProgressBar, RichText,
    ScrollArea, Stroke, TextEdit, ViewportBuilder,
};
use std::{
    any::Any,
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

mod actions;
mod diagnostics;
mod events;
mod helpers;
mod jobs;
mod shell;
mod state;
mod views;

use helpers::*;
use state::*;

pub fn run() -> eframe::Result<()> {
    if let Ok(log_path) = logging::init() {
        logging::info(format!(
            "GUI run requested; log path {}",
            log_path.display()
        ));
    }
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([1480.0, 920.0])
            .with_min_inner_size([1024.0, 720.0])
            .with_maximized(true)
            .with_title("Soziopolis Reader"),
        ..Default::default()
    };

    eframe::run_native(
        "Soziopolis Reader",
        options,
        Box::new(|cc| Ok(Box::new(SoziopolisLingqGui::new(cc)))),
    )
}
