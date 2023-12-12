/// This module is used for the indexing from any source.
pub mod local_import;

use std::collections::HashMap;

use crate::database::redis::RedisPool;
use crate::models::ids::base62_impl::to_base62;
use crate::search::{SearchConfig, UploadSearchProject};
use local_import::index_local;
use log::info;
use meilisearch_sdk::client::Client;
use meilisearch_sdk::indexes::Index;
use meilisearch_sdk::settings::{PaginationSetting, Settings};
use sqlx::postgres::PgPool;
use thiserror::Error;

use self::local_import::get_all_ids;

#[derive(Error, Debug)]
pub enum IndexingError {
    #[error("Error while connecting to the MeiliSearch database")]
    Indexing(#[from] meilisearch_sdk::errors::Error),
    #[error("Error while serializing or deserializing JSON: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Database Error: {0}")]
    Sqlx(#[from] sqlx::error::Error),
    #[error("Database Error: {0}")]
    Database(#[from] crate::database::models::DatabaseError),
    #[error("Environment Error")]
    Env(#[from] dotenvy::Error),
    #[error("Error while awaiting index creation task")]
    Task,
}

// The chunk size for adding projects to the indexing database. If the request size
// is too large (>10MiB) then the request fails with an error.  This chunk size
// assumes a max average size of 4KiB per project to avoid this cap.
const MEILISEARCH_CHUNK_SIZE: usize = 2500; // Should be less than FETCH_PROJECT_SIZE

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

pub async fn remove_documents(
    ids: &[crate::models::ids::VersionId],
    config: &SearchConfig,
) -> Result<(), meilisearch_sdk::errors::Error> {
    let indexes = get_indexes(config).await?;

    for index in indexes {
        index
            .delete_documents(&ids.iter().map(|x| to_base62(x.0)).collect::<Vec<_>>())
            .await?;
    }

    Ok(())
}

pub async fn index_projects(
    pool: PgPool,
    redis: RedisPool,
    config: &SearchConfig,
) -> Result<(), IndexingError> {
    info!("Indexing projects.");

    let all_ids = get_all_ids(pool.clone()).await?;

    info!("Ids returned.");
    let indices = get_indexes(config).await?;
    info!("Retrived Indexes.");
    let (uploads, loader_fields) = index_local(
        &pool,
        &redis,
        all_ids
            .into_iter()
            .map(|(version_id, project_id, owner_username)| {
                (version_id, (project_id, owner_username.to_lowercase()))
            })
            .collect::<HashMap<_, _>>(),
    )
    .await?;
    info!("Indexed local.");
    add_projects(&indices, uploads, loader_fields, config).await?;

    info!("Done adding projects.");
    Ok(())
}

pub async fn get_indexes(
    config: &SearchConfig,
) -> Result<Vec<Index>, meilisearch_sdk::errors::Error> {
    let client = config.make_client();

    let projects_index = create_or_update_index(&client, "projects", None).await?;
    let projects_filtered_index = create_or_update_index(
        &client,
        "projects_filtered",
        Some(&[
            "sort",
            "words",
            "typo",
            "proximity",
            "attribute",
            "exactness",
        ]),
    )
    .await?;

    Ok(vec![projects_index, projects_filtered_index])
}

async fn create_or_update_index(
    client: &Client,
    name: &'static str,
    custom_rules: Option<&'static [&'static str]>,
) -> Result<Index, meilisearch_sdk::errors::Error> {
    info!("Updating/creating index.");

    match client.get_index(name).await {
        Ok(index) => {
            info!("Updating index settings.");

            let mut settings = default_settings();

            if let Some(custom_rules) = custom_rules {
                settings = settings.with_ranking_rules(custom_rules);
            }

            index
                .set_settings(&settings)
                .await?
                .wait_for_completion(client, None, Some(TIMEOUT))
                .await?;
            Ok(index)
        }
        _ => {
            info!("Creating index.");

            // Only create index and set settings if the index doesn't already exist
            let task = client.create_index(name, Some("version_id")).await?;
            let task = task
                .wait_for_completion(client, None, Some(TIMEOUT))
                .await?;
            let index = task
                .try_make_index(client)
                .map_err(|x| x.unwrap_failure())?;

            let mut settings = default_settings();

            if let Some(custom_rules) = custom_rules {
                settings = settings.with_ranking_rules(custom_rules);
            }

            index
                .set_settings(&settings)
                .await?
                .wait_for_completion(client, None, Some(TIMEOUT))
                .await?;

            Ok(index)
        }
    }
}

async fn add_to_index(
    client: &Client,
    index: &Index,
    mods: &[UploadSearchProject],
) -> Result<(), IndexingError> {
    for chunk in mods.chunks(MEILISEARCH_CHUNK_SIZE) {
        info!(
            "Adding chunk starting with version id {}",
            chunk[0].version_id
        );
        index
            .add_or_replace(chunk, Some("version_id"))
            .await?
            .wait_for_completion(client, None, Some(std::time::Duration::from_secs(3600)))
            .await?;
        info!("Added chunk of {} projects to index", chunk.len());
    }

    Ok(())
}

async fn update_and_add_to_index(
    client: &Client,
    index: &Index,
    projects: &[UploadSearchProject],
    additional_fields: &[String],
) -> Result<(), IndexingError> {
    let mut new_filterable_attributes: Vec<String> = index.get_filterable_attributes().await?;
    let mut new_displayed_attributes = index.get_displayed_attributes().await?;

    new_filterable_attributes.extend(additional_fields.iter().map(|s| s.to_string()));
    new_displayed_attributes.extend(additional_fields.iter().map(|s| s.to_string()));
    info!("add attributes.");
    index
        .set_filterable_attributes(new_filterable_attributes)
        .await?;
    index
        .set_displayed_attributes(new_displayed_attributes)
        .await?;

    info!("Adding to index.");

    add_to_index(client, index, projects).await?;
    Ok(())
}

pub async fn add_projects(
    indices: &[Index],
    projects: Vec<UploadSearchProject>,
    additional_fields: Vec<String>,
    config: &SearchConfig,
) -> Result<(), IndexingError> {
    let client = config.make_client();
    for index in indices {
        info!("adding projects part1 or 2.");
        update_and_add_to_index(&client, index, &projects, &additional_fields).await?;
    }

    Ok(())
}

fn default_settings() -> Settings {
    Settings::new()
        .with_distinct_attribute("project_id")
        .with_displayed_attributes(DEFAULT_DISPLAYED_ATTRIBUTES)
        .with_searchable_attributes(DEFAULT_SEARCHABLE_ATTRIBUTES)
        .with_sortable_attributes(DEFAULT_SORTABLE_ATTRIBUTES)
        .with_filterable_attributes(DEFAULT_ATTRIBUTES_FOR_FACETING)
        .with_pagination(PaginationSetting {
            max_total_hits: 2147483647,
        })
}

const DEFAULT_DISPLAYED_ATTRIBUTES: &[&str] = &[
    "project_id",
    "version_id",
    "project_types",
    "slug",
    "author",
    "name",
    "summary",
    "categories",
    "display_categories",
    "downloads",
    "follows",
    "icon_url",
    "date_created",
    "date_modified",
    "latest_version",
    "license",
    "gallery",
    "featured_gallery",
    "color",
    // Note: loader fields are not here, but are added on as they are needed (so they can be dynamically added depending on which exist).

    // Non-searchable fields for filling out the Project model.
    "license_url",
    "monetization_status",
    "team_id",
    "thread_id",
    "versions",
    "date_published",
    "date_queued",
    "status",
    "requested_status",
    "games",
    "organization_id",
    "links",
    "gallery_items",
    "loaders", // search uses loaders as categories- this is purely for the Project model.
];

const DEFAULT_SEARCHABLE_ATTRIBUTES: &[&str] = &["name", "summary", "author", "slug"];

const DEFAULT_ATTRIBUTES_FOR_FACETING: &[&str] = &[
    "categories",
    "license",
    "project_types",
    "downloads",
    "follows",
    "author",
    "name",
    "date_created",
    "created_timestamp",
    "date_modified",
    "modified_timestamp",
    "project_id",
    "open_source",
    "color",
];

const DEFAULT_SORTABLE_ATTRIBUTES: &[&str] =
    &["downloads", "follows", "date_created", "date_modified"];
