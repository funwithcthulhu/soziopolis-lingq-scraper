use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;

const LINGQ_BASE: &str = "https://www.lingq.com/api/v3";
const LINGQ_AUTH: &str = "https://www.lingq.com/api/v2/api-token-auth/";
const LINGQ_TIMEOUT: Duration = Duration::from_secs(20);
const LINGQ_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct Collection {
    pub id: i64,
    pub title: String,
    pub lessons_count: i64,
}

#[derive(Debug, Clone)]
pub struct UploadRequest {
    pub api_key: String,
    pub language_code: String,
    pub collection_id: Option<i64>,
    pub title: String,
    pub text: String,
    pub original_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadResponse {
    pub lesson_id: i64,
    pub lesson_url: String,
}

#[derive(Debug, Clone)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Deserialize)]
struct LingqLessonResponse {
    id: i64,
}

#[derive(Deserialize)]
struct LingqTokenResponse {
    token: Option<String>,
}

#[derive(Deserialize)]
struct LingqCollectionsResponse {
    results: Vec<LingqCollectionRow>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct LingqCollectionRow {
    id: i64,
    title: String,
    #[serde(rename = "lessonsCount")]
    lessons_count: Option<i64>,
    #[serde(rename = "lessons_count")]
    lessons_count_alt: Option<i64>,
}

pub struct LingqClient {
    client: Client,
}

impl LingqClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(LINGQ_CONNECT_TIMEOUT)
            .timeout(LINGQ_TIMEOUT)
            .build()
            .context("failed to build LingQ HTTP client")?;

        Ok(Self { client })
    }

    pub fn login(&self, username: &str, password: &str) -> Result<LoginResponse> {
        let params = [("username", username), ("password", password)];
        let response = self
            .client
            .post(LINGQ_AUTH)
            .form(&params)
            .send()
            .context("LingQ login request failed")?;
        let payload: LingqTokenResponse = parse_json_response(
            response,
            "LingQ rejected the username/password login",
            "LingQ login response",
        )?;

        let token = payload
            .token
            .filter(|token| !token.trim().is_empty())
            .context("LingQ login succeeded but no token was returned")?;

        Ok(LoginResponse { token })
    }

    pub fn get_collections(&self, api_key: &str, language_code: &str) -> Result<Vec<Collection>> {
        let mut collections = Vec::new();
        let mut next_url = Some(format!("{}/{}/collections/my/", LINGQ_BASE, language_code));

        while let Some(url) = next_url.take() {
            let response = self
                .client
                .get(&url)
                .header("Authorization", format!("Token {api_key}"))
                .send()
                .with_context(|| format!("LingQ collections request failed for {url}"))?;
            let payload: LingqCollectionsResponse = parse_json_response(
                response,
                "LingQ rejected the API key or collections request",
                "LingQ collections response",
            )?;

            collections.extend(payload.results.into_iter().map(|row| Collection {
                id: row.id,
                title: row.title,
                lessons_count: row.lessons_count.or(row.lessons_count_alt).unwrap_or(0),
            }));
            next_url = payload.next;
        }

        collections.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        Ok(collections)
    }

    pub fn upload_lesson(&self, request: &UploadRequest) -> Result<UploadResponse> {
        let normalized_text = normalize_text(&request.text);
        if normalized_text.trim().is_empty() {
            bail!("lesson text is empty");
        }

        let mut payload = serde_json::json!({
            "title": request.title,
            "text": normalized_text,
            "status": "private",
        });

        if let Some(collection_id) = request.collection_id {
            payload["collection"] = serde_json::json!(collection_id);
        }

        if let Some(original_url) = &request.original_url {
            payload["original_url"] = serde_json::json!(original_url);
        }

        let response = self
            .client
            .post(format!("{}/{}/lessons/", LINGQ_BASE, request.language_code))
            .header("Authorization", format!("Token {}", request.api_key))
            .json(&payload)
            .send()
            .context("LingQ upload request failed")?;
        let lesson: LingqLessonResponse = parse_json_response(
            response,
            "LingQ rejected the lesson upload",
            "LingQ upload response",
        )?;

        Ok(UploadResponse {
            lesson_id: lesson.id,
            lesson_url: format!(
                "https://www.lingq.com/{}/learn/lesson/{}/",
                request.language_code, lesson.id
            ),
        })
    }
}

fn parse_json_response<T>(
    response: reqwest::blocking::Response,
    rejection_context: &str,
    parse_context: &str,
) -> Result<T>
where
    T: DeserializeOwned,
{
    let status = response.status();
    let body = response
        .text()
        .with_context(|| format!("failed to read {parse_context} body"))?;

    if !status.is_success() {
        let body_summary = summarize_api_body(&body);
        return Err(anyhow!(
            "{} (status {}).{}",
            rejection_context,
            status,
            if body_summary.is_empty() {
                String::new()
            } else {
                format!(" Response: {body_summary}")
            }
        ));
    }

    serde_json::from_str(&body).with_context(|| {
        let body_summary = summarize_api_body(&body);
        if body_summary.is_empty() {
            format!("failed to parse {parse_context}")
        } else {
            format!("failed to parse {parse_context}: {body_summary}")
        }
    })
}

fn normalize_text(text: &str) -> String {
    text.split("\n\n")
        .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn summarize_api_body(body: &str) -> String {
    let condensed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if condensed.is_empty() {
        return String::new();
    }

    if condensed.chars().count() <= 220 {
        condensed
    } else {
        format!("{}...", condensed.chars().take(217).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::{Collection, normalize_text, summarize_api_body};

    #[test]
    fn normalize_text_preserves_paragraph_breaks() {
        let text = " One   two \n\n  Three   four ";
        assert_eq!(normalize_text(text), "One two\n\nThree four");
    }

    #[test]
    fn summarize_api_body_condenses_whitespace_and_truncates() {
        let input = format!("error: {}", "detail ".repeat(80));
        let summary = summarize_api_body(&input);
        assert!(summary.starts_with("error: detail"));
        assert!(summary.ends_with("..."));
        assert!(summary.len() <= 220);
    }

    #[test]
    fn collections_are_sorted_case_insensitively() {
        let mut collections = [
            Collection {
                id: 1,
                title: "zebra".to_owned(),
                lessons_count: 0,
            },
            Collection {
                id: 2,
                title: "Alpha".to_owned(),
                lessons_count: 0,
            },
        ];
        collections.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        assert_eq!(collections[0].title, "Alpha");
        assert_eq!(collections[1].title, "zebra");
    }
}
