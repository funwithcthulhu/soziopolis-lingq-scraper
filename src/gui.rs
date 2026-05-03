use crate::{
    app_error::AppError,
    app_ops, app_paths,
    context::AppContext,
    credential_store,
    database::{LibraryStats, SectionCount, StoredArticle},
    domain::{ArticleListItem, ArticleListPage, LibraryPageRequest, LibraryQuery, LibrarySortMode},
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

use iced::widget::{
    Column, Row, Space, button, checkbox, container, horizontal_rule, horizontal_space, pick_list,
    progress_bar, row, scrollable, text, text_input,
};
// Use a macro alias to avoid ambiguity with std::column! in Rust 2024 edition
macro_rules! wcolumn {
    ($($arg:tt)*) => { iced::widget::column![$($arg)*] };
}
use iced::{Element, Length, Subscription, Task, Theme, clipboard};

use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    fs,
    path::PathBuf,
    process::Command as SysCommand,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

mod helpers;
mod message;
mod state;
mod style;
mod tasks;
mod update;
mod views;

use helpers::*;
use message::*;
use state::*;
use style::*;

const LIBRARY_PAGE_SIZE: usize = 60;

pub fn run() -> iced::Result {
    iced::application("Soziopolis Reader", App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size(iced::Size::new(1480.0, 920.0))
        .run_with(App::new)
}
