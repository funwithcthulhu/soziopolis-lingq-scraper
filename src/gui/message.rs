use super::*;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) enum Message {
    // Navigation
    SwitchView(View),
    ToggleLingqSettings,
    ClosePreview,

    // Browse
    BrowseSectionChanged(String),
    BrowseSearchChanged(String),
    BrowseToggleOnlyNew(bool),
    BrowseToggleArticle(String),
    BrowseRefresh,
    BrowseAllSections,
    BrowseLoadMore,
    BrowseSelectVisibleNew,
    BrowseClearSelection,
    BrowseFetchSelected,
    BrowseFindNew,
    BrowseLoaded {
        request_id: u64,
        result: Result<BrowseResponse, AppError>,
    },

    // Preview
    OpenPreview(String),
    OpenLibraryPreview(i64),
    PreviewLoaded(Result<(Article, Option<StoredArticle>), AppError>),
    OpenFullArticle(i64),

    // Library
    LibrarySearchChanged(String),
    LibraryTopicChanged(String),
    LibraryToggleNotUploaded(bool),
    LibraryMinWordsChanged(String),
    LibraryMaxWordsChanged(String),
    LibrarySortChanged(LibrarySortMode),
    LibraryToggleDense(bool),
    LibraryToggleGroupByTopic(bool),
    LibraryToggleFilters,
    LibraryRefresh,
    LibrarySelectAllVisible,
    LibrarySelectAllNotUploaded,
    LibraryClearSelection,
    LibraryToggleArticle(i64),
    LibraryDeleteArticle(i64),
    LibraryNextPage,
    LibraryPrevPage,

    // Article detail
    ArticleBack,
    ArticleCopyText,
    OpenArticle(i64),

    // LingQ auth
    LingqAuthModeChanged(LingqAuthMode),
    LingqUsernameChanged(String),
    LingqPasswordChanged(String),
    LingqApiKeyChanged(String),
    LingqConnect,
    LingqDisconnect,
    LingqSignIn,
    LingqCollectionChanged(Option<i64>),
    LingqRefreshCollections,
    LingqLoggedIn(Result<String, AppError>),
    CollectionsLoaded(Result<Vec<Collection>, AppError>),

    // LingQ upload selection
    LingqClearUploadSelection,
    LingqUploadSelected,

    // Background task results
    ImportProgress(ImportProgress),
    BatchFetched {
        job_id: u64,
        saved_count: usize,
        saved_articles: Vec<ArticleListItem>,
        skipped_existing: usize,
        skipped_out_of_range: usize,
        failed: Vec<FailedFetchItem>,
        canceled: bool,
    },
    UploadProgressMsg {
        job_id: u64,
        progress: UploadProgress,
    },
    BatchUploaded {
        job_id: u64,
        uploaded: usize,
        successes: Vec<UploadSuccess>,
        failed: Vec<UploadFailure>,
        canceled: bool,
    },
    ContentRefreshCompleted {
        request_id: u64,
        reason: String,
        result: ContentRefreshResult,
    },

    // Job queue
    CancelActiveJob,
    PauseQueue,
    ResumeQueue,
    RunQueuedUploadNow,
    ClearQueuedJobs,
    RetryFailedImports,
    RetryFailedUploads,

    // Diagnostics
    SelectDiagnosticsJob(u64),
    OpenDataFolder,
    OpenLogFile,
    CopyRecentLog,
    CreateSupportBundle,
    ClearBrowseCache,
    CompactLocalData,
    RebuildSearchIndex,
    VerifyDatabase,
    ClearTaskFailures,

    // Misc
    NoticeExpired,
    OpenUrl(String),
    Tick,
    Noop,
}
