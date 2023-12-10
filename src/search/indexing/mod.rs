/// This module is used for the indexing from any source.
pub mod local_import;

use std::collections::HashMap;

use crate::database::redis::RedisPool;
use crate::search::{SearchConfig, UploadSearchProject};
use itertools::Itertools;
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
// assumes a max average size of 1KiB per project to avoid this cap.
const MEILISEARCH_CHUNK_SIZE: usize = 100;

const FETCH_PROJECT_SIZE: usize = 5000;
pub async fn index_projects(
    pool: PgPool,
    redis: RedisPool,
    config: &SearchConfig,
) -> Result<(), IndexingError> {
    let mut docs_to_add: Vec<UploadSearchProject> = vec![];
    let mut additional_fields: Vec<String> = vec![];

    let all_ids = get_all_ids(pool.clone()).await?;
    let all_ids_len = all_ids.len();
    info!("Got all ids, indexing {} projects", all_ids_len);
    let mut so_far = 0;

    let as_chunks: Vec<_> = all_ids
        .into_iter()
        .chunks(FETCH_PROJECT_SIZE)
        .into_iter()
        .map(|x| x.collect::<Vec<_>>())
        .collect();

    for id_chunk in as_chunks {
        info!(
            "Fetching chunk {}-{}/{}, size: {}",
            so_far,
            so_far + FETCH_PROJECT_SIZE,
            all_ids_len,
            id_chunk.len()
        );
        so_far += FETCH_PROJECT_SIZE;

        let id_chunk = id_chunk
            .into_iter()
            .map(|(version_id, project_id, owner_username)| {
                (version_id, (project_id, owner_username.to_lowercase()))
            })
            .collect::<HashMap<_, _>>();
        let (mut uploads, mut loader_fields) = index_local(&pool, &redis, id_chunk).await?;
        // docs_to_add.append(&mut uploads);
        // additional_fields.append(&mut loader_fields);


        info!("Adding chunk to index...");
        // Write Indices
        add_projects(uploads, loader_fields, config).await?;

        // docs_to_add.clear();
        // additional_fields.clear();
    }

    info!("Done adding projects.");
    Ok(())
}

async fn create_index(
    client: &Client,
    name: &'static str,
    custom_rules: Option<&'static [&'static str]>,
) -> Result<Index, IndexingError> {
    info!("Creating index {}", name);
    client
        .delete_index(name)
        .await?
        .wait_for_completion(client, None, None)
        .await?;
    info!("Deleted index {}", name);
    match client.get_index(name).await {
        Ok(index) => {
            index
                .set_settings(&default_settings())
                .await?
                .wait_for_completion(client, None, None)
                .await?;
            info!("C1reated index {}", name);
            Ok(index)
        }
        Err(meilisearch_sdk::errors::Error::Meilisearch(
            meilisearch_sdk::errors::MeilisearchError {
                error_code: meilisearch_sdk::errors::ErrorCode::IndexNotFound,
                ..
            },
        )) => {
            // Only create index and set settings if the index doesn't already exist
            let task = client.create_index(name, Some("version_id")).await?;
            let task = task.wait_for_completion(client, None, None).await?;
            let index = task
                .try_make_index(client)
                .map_err(|_| IndexingError::Task)?;

            let mut settings = default_settings();

            if let Some(custom_rules) = custom_rules {
                settings = settings.with_ranking_rules(custom_rules);
            }

            index
                .set_settings(&settings)
                .await?
                .wait_for_completion(client, None, None)
                .await?;

            info!("C2reated index {}", name);
            Ok(index)
        }
        Err(e) => {
            log::warn!("Unhandled error while creating index: {}", e);
            Err(IndexingError::Indexing(e))
        }
    }
}

async fn add_to_index(
    client: &Client,
    index: Index,
    mods: &[UploadSearchProject],
) -> Result<(), IndexingError> {
    for chunk in mods.chunks(MEILISEARCH_CHUNK_SIZE) {
        println!("Adding chunk of size {}", chunk.len());
        let r = index
            .add_documents(chunk.clone(), Some("version_id"))
            .await;
            if r.is_err() {
                log::warn!("1Error while adding documents: {:?}", chunk.into_iter().map(|x| x.project_id.to_string()).collect_vec());
            }
        let r = r?.wait_for_completion(client, None, None)
            .await;
        if r.is_err() {
            log::warn!("2Error while adding documents: {:?}", chunk.into_iter().map(|x| x.project_id.to_string()).collect_vec());
        }
        r?;
    }
    Ok(())
}

async fn create_and_add_to_index(
    client: &Client,
    projects: &[UploadSearchProject],
    additional_fields: &[String],
    name: &'static str,
    custom_rules: Option<&'static [&'static str]>,
) -> Result<(), IndexingError> {
    let index = create_index(client, name, custom_rules).await?;

    let mut new_filterable_attributes = index.get_filterable_attributes().await?;
    let mut new_displayed_attributes = index.get_displayed_attributes().await?;

    new_filterable_attributes.extend(additional_fields.iter().map(|s| s.to_string()));
    new_displayed_attributes.extend(additional_fields.iter().map(|s| s.to_string()));
    index
        .set_filterable_attributes(new_filterable_attributes)
        .await?;
    index
        .set_displayed_attributes(new_displayed_attributes)
        .await?;

    add_to_index(client, index, projects).await?;
    Ok(())
}

pub async fn add_projects(
    projects: Vec<UploadSearchProject>,
    additional_fields: Vec<String>,
    config: &SearchConfig,
) -> Result<(), IndexingError> {
    let client = config.make_client();

    create_and_add_to_index(&client, &projects, &additional_fields, "projects", None).await?;

    create_and_add_to_index(
        &client,
        &projects,
        &additional_fields,
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
