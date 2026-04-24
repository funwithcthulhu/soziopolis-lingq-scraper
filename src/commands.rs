use crate::{
    context::AppContext,
    domain::{ArticleListItem, ArticleListPage, LibrarySortMode},
    repositories::ArticleRepository,
    services::ContentRefreshResult,
    soziopolis,
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

pub fn list_library_cards_page(
    ctx: &AppContext,
    search: Option<&str>,
    section: Option<&str>,
    only_not_uploaded: bool,
    min_words: Option<usize>,
    max_words: Option<usize>,
    sort_mode: LibrarySortMode,
    offset: usize,
    limit: usize,
) -> Result<ArticleListPage> {
    ctx.db.with_db(|db| {
        let repository = ArticleRepository::new(db);
        repository.list_article_cards_page(
            search,
            section,
            only_not_uploaded,
            min_words,
            max_words,
            sort_mode,
            offset,
            limit,
        )
    })
}

pub fn list_matching_library_card_ids(
    ctx: &AppContext,
    search: Option<&str>,
    section: Option<&str>,
    only_not_uploaded: bool,
    min_words: Option<usize>,
    max_words: Option<usize>,
) -> Result<Vec<i64>> {
    ctx.db.with_db(|db| {
        let repository = ArticleRepository::new(db);
        repository.list_matching_article_card_ids(
            search,
            section,
            only_not_uploaded,
            min_words,
            max_words,
        )
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
