use crate::{
    context::AppContext, domain::ArticleListItem, repositories::ArticleRepository,
    services::ContentRefreshResult, soziopolis,
};
use anyhow::Result;

pub fn refresh_content(ctx: &AppContext) -> Result<ContentRefreshResult> {
    Ok(crate::services::LibraryService::refresh_content(ctx))
}

pub fn search_library_cards(
    ctx: &AppContext,
    search: Option<&str>,
    section: Option<&str>,
    only_not_uploaded: bool,
) -> Result<Vec<ArticleListItem>> {
    ctx.db.with_db(|db| {
        let repository = ArticleRepository::new(db);
        repository.list_article_cards(search, section, only_not_uploaded)
    })
}

pub fn get_article_detail(
    ctx: &AppContext,
    id: i64,
) -> Result<Option<crate::database::StoredArticle>> {
    crate::services::LibraryService::get_article(ctx, id)
}

pub fn delete_article(ctx: &AppContext, id: i64) -> Result<()> {
    crate::services::LibraryService::delete_article(ctx, id)
}

pub fn compact_local_data(ctx: &AppContext) -> Result<()> {
    ctx.db.with_db(|db| db.compact_storage())
}

pub fn rebuild_search_index(ctx: &AppContext) -> Result<()> {
    ctx.db.with_db(|db| db.rebuild_search_index())
}

pub fn verify_database(ctx: &AppContext) -> Result<String> {
    ctx.db.with_db(|db| db.integrity_check())
}

pub fn clear_browse_cache() -> Result<usize> {
    soziopolis::clear_browse_cache()
}
