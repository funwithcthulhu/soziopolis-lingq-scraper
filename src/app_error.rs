use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppErrorKind {
    Network,
    Parse,
    Database,
    Auth,
    Upload,
    Validation,
    Internal,
    External,
    Unknown,
}

impl AppErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Network => "Network",
            Self::Parse => "Parse",
            Self::Database => "Database",
            Self::Auth => "Authentication",
            Self::Upload => "Upload",
            Self::Validation => "Validation",
            Self::Internal => "Internal",
            Self::External => "External",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub kind: AppErrorKind,
    pub operation: String,
    pub message: String,
    pub details: Option<String>,
    pub recorded_at: String,
}

impl AppError {
    pub fn new(
        kind: AppErrorKind,
        operation: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation: operation.into(),
            message: message.into(),
            details: None,
            recorded_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_owned()),
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn classify(operation: impl Into<String>, message: impl Into<String>) -> Self {
        let operation = operation.into();
        let message = message.into();
        let lower = message.to_lowercase();

        let kind = if lower.contains("unauthorized")
            || lower.contains("forbidden")
            || lower.contains("invalid credentials")
            || lower.contains("api key")
            || lower.contains("password")
            || lower.contains("token")
        {
            AppErrorKind::Auth
        } else if lower.contains("upload") || lower.contains("lesson") || lower.contains("lingq") {
            AppErrorKind::Upload
        } else if lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("connection")
            || lower.contains("dns")
            || lower.contains("http")
            || lower.contains("429")
            || lower.contains("503")
        {
            AppErrorKind::Network
        } else if lower.contains("sqlite")
            || lower.contains("database")
            || lower.contains("sql")
            || lower.contains("wal")
        {
            AppErrorKind::Database
        } else if lower.contains("parse")
            || lower.contains("selector")
            || lower.contains("html")
            || lower.contains("missing title")
        {
            AppErrorKind::Parse
        } else if lower.contains("must")
            || lower.contains("required")
            || lower.contains("invalid")
            || lower.contains("not found")
        {
            AppErrorKind::Validation
        } else {
            AppErrorKind::Unknown
        };

        Self::new(kind, operation, message)
    }

    pub fn internal_task(task_kind: &str, task_label: &str, message: impl Into<String>) -> Self {
        Self::new(
            AppErrorKind::Internal,
            format!("{task_kind} task"),
            message.into(),
        )
        .with_details(format!("Task label: {task_label}"))
    }

    pub fn notice_message(&self) -> String {
        format!("{} failed: {}", self.operation, self.message)
    }
}
