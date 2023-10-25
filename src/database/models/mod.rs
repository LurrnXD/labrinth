use thiserror::Error;

pub mod categories;
pub mod collection_item;
pub mod creator_follows;
pub mod event_item;
pub mod flow_item;
pub mod ids;
pub mod image_item;
pub mod notification_item;
pub mod oauth_client_authorization_item;
pub mod oauth_client_item;
pub mod oauth_token_item;
pub mod organization_item;
pub mod pat_item;
pub mod project_item;
pub mod report_item;
pub mod session_item;
pub mod team_item;
pub mod thread_item;
pub mod user_item;
pub mod version_item;

pub use collection_item::Collection;
pub use event_item::Event;
pub use ids::*;
pub use image_item::Image;
pub use organization_item::Organization;
pub use project_item::Project;
pub use team_item::Team;
pub use team_item::TeamMember;
pub use thread_item::{Thread, ThreadMessage};
pub use user_item::User;
pub use version_item::Version;

use self::dynamic::IdType;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Error while interacting with the database: {0}")]
    Database(#[from] sqlx::Error),
    #[error(
        "Error converting from a dynamic id in the database (expected {expected:#?}, was {actual:#?})"
    )]
    DynamicIdConversionError { expected: IdType, actual: IdType },
    #[error("Didn't expect value to be null: {0}")]
    UnexpectedNull(String),
    #[error("Error while trying to generate random ID")]
    RandomId,
    #[error("Error while interacting with the cache: {0}")]
    CacheError(#[from] redis::RedisError),
    #[error("Redis Pool Error: {0}")]
    RedisPool(#[from] deadpool_redis::PoolError),
    #[error("Error while serializing with the cache: {0}")]
    SerdeCacheError(#[from] serde_json::Error),
}
