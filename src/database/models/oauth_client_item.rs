use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::models::pats::Scopes;

use super::{DatabaseError, OAuthClientId, OAuthRedirectUriId, UserId};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthRedirectUri {
    pub id: OAuthRedirectUriId,
    pub client_id: OAuthClientId,
    pub uri: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OAuthClient {
    pub id: OAuthClientId,
    pub name: String,
    pub icon_url: Option<String>,
    pub max_scopes: Scopes,
    pub secret_hash: String,
    pub redirect_uris: Vec<OAuthRedirectUri>,
    pub created: DateTime<Utc>,
    pub created_by: UserId,
}

struct ClientQueryResult {
    id: i64,
    name: String,
    icon_url: Option<String>,
    max_scopes: i64,
    secret_hash: String,
    created: DateTime<Utc>,
    created_by: i64,
    uri_ids: Option<Vec<i64>>,
    uri_vals: Option<Vec<String>>,
}

macro_rules! select_clients_with_predicate {
    ($predicate:tt, $param:ident) => {
        sqlx::query_as!(
            ClientQueryResult,
            "
            SELECT
                clients.id,
                clients.name,
                clients.icon_url,
                clients.max_scopes,
                clients.secret_hash,
                clients.created,
                clients.created_by,
                uris.uri_ids,
                uris.uri_vals
            FROM oauth_clients clients
            LEFT JOIN (
                SELECT client_id, array_agg(id) as uri_ids, array_agg(uri) as uri_vals
                FROM oauth_client_redirect_uris
                GROUP BY client_id
            ) uris ON clients.id = uris.client_id
            "
                + $predicate,
            $param
        )
    };
}

impl OAuthClient {
    pub async fn get(
        id: OAuthClientId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Option<OAuthClient>, DatabaseError> {
        let client_id_param = id.0;
        let value = select_clients_with_predicate!("WHERE clients.id = $1", client_id_param)
            .fetch_optional(exec)
            .await?;

        return Ok(value.map(|r| r.into()));
    }

    pub async fn get_all_user_clients(
        user_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<OAuthClient>, DatabaseError> {
        let user_id_param = user_id.0;
        let clients = select_clients_with_predicate!("WHERE created_by = $1", user_id_param)
            .fetch_all(exec)
            .await?;

        return Ok(clients.into_iter().map(|r| r.into()).collect());
    }

    pub async fn remove(
        id: OAuthClientId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        // Cascades to oauth_client_redirect_uris, oauth_client_authorizations
        sqlx::query!(
            "
            DELETE FROM oauth_clients
            WHERE id = $1
            ",
            id.0
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    pub async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO oauth_clients (
                id, name, icon_url, max_scopes, secret_hash, created_by
            )
            VALUES (
                $1, $2, $3, $4, $5, $6
            )
            ",
            self.id.0,
            self.name,
            self.icon_url,
            self.max_scopes.to_postgres(),
            self.secret_hash,
            self.created_by.0
        )
        .execute(&mut *transaction)
        .await?;

        Self::insert_redirect_uris(&self.redirect_uris, transaction).await?;

        Ok(())
    }

    pub async fn update_editable_fields(
        &self,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            UPDATE oauth_clients
            SET name = $1, icon_url = $2, max_scopes = $3
            WHERE (id = $4)
            ",
            self.name,
            self.icon_url,
            self.max_scopes.to_postgres(),
            self.id.0,
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    pub async fn remove_redirect_uris(
        ids: impl IntoIterator<Item = OAuthRedirectUriId>,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        let ids = ids.into_iter().map(|id| id.0).collect_vec();
        sqlx::query!(
            "
            DELETE FROM oauth_clients
            WHERE id IN
            (SELECT * FROM UNNEST($1::bigint[]))
            ",
            &ids[..]
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    pub async fn insert_redirect_uris(
        uris: &[OAuthRedirectUri],
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        let (ids, client_ids, uris): (Vec<_>, Vec<_>, Vec<_>) = uris
            .iter()
            .map(|r| (r.id.0, r.client_id.0, r.uri.clone()))
            .multiunzip();
        sqlx::query!(
            "
            INSERT INTO oauth_client_redirect_uris (id, client_id, uri)
            SELECT * FROM UNNEST($1::bigint[], $2::bigint[], $3::varchar[])
            ",
            &ids[..],
            &client_ids[..],
            &uris[..],
        )
        .execute(exec)
        .await?;

        Ok(())
    }
}

impl From<ClientQueryResult> for OAuthClient {
    fn from(r: ClientQueryResult) -> Self {
        let redirects = if let (Some(ids), Some(uris)) = (r.uri_ids.as_ref(), r.uri_vals.as_ref()) {
            ids.iter()
                .zip(uris.iter())
                .map(|(id, uri)| OAuthRedirectUri {
                    id: OAuthRedirectUriId(*id),
                    client_id: OAuthClientId(r.id.clone()),
                    uri: uri.to_string(),
                })
                .collect()
        } else {
            vec![]
        };

        OAuthClient {
            id: OAuthClientId(r.id),
            name: r.name,
            icon_url: r.icon_url,
            max_scopes: Scopes::from_postgres(r.max_scopes),
            secret_hash: r.secret_hash,
            redirect_uris: redirects,
            created: r.created,
            created_by: UserId(r.created_by),
        }
    }
}
