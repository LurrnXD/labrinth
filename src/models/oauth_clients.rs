use super::{
    ids::{Base62Id, UserId},
    pats::Scopes,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::database::models::oauth_client_authorization_item::OAuthClientAuthorization as DBOAuthClientAuthorization;
use crate::database::models::oauth_client_item::OAuthClient as DBOAuthClient;
use crate::database::models::oauth_client_item::OAuthRedirectUri as DBOAuthRedirectUri;

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct OAuthClientId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct OAuthClientAuthorizationId(pub u64);

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Base62Id")]
#[serde(into = "Base62Id")]
pub struct OAuthRedirectUriId(pub u64);

#[derive(Deserialize, Serialize)]
pub struct OAuthRedirectUri {
    pub id: OAuthRedirectUriId,
    pub client_id: OAuthClientId,
    pub uri: String,
}

#[derive(Serialize, Deserialize)]
pub struct OAuthClientCreationResult {
    pub client_secret: String,
    pub client: OAuthClient,
}

#[derive(Deserialize, Serialize)]
pub struct OAuthClient {
    pub id: OAuthClientId,
    pub name: String,
    pub icon_url: Option<String>,

    // The maximum scopes the client can request for OAuth
    pub max_scopes: Scopes,

    // The valid URIs that can be redirected to during an authorization request
    pub redirect_uris: Vec<OAuthRedirectUri>,

    // The user that created (and thus controls) this client
    pub created_by: UserId,
}

#[derive(Deserialize, Serialize)]
pub struct OAuthClientAuthorization {
    pub id: OAuthClientAuthorizationId,
    pub app_id: OAuthClientId,
    pub user_id: UserId,
    pub scopes: Scopes,
    pub created: DateTime<Utc>,
}

#[derive(Deserialize, Serialize)]
pub struct GetOAuthClientsRequest {
    pub ids: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct DeleteOAuthClientQueryParam {
    pub client_id: String,
}

impl From<DBOAuthClient> for OAuthClient {
    fn from(value: DBOAuthClient) -> Self {
        Self {
            id: value.id.into(),
            name: value.name,
            icon_url: value.icon_url,
            max_scopes: value.max_scopes,
            redirect_uris: value.redirect_uris.into_iter().map(|r| r.into()).collect(),
            created_by: value.created_by.into(),
        }
    }
}

impl From<DBOAuthRedirectUri> for OAuthRedirectUri {
    fn from(value: DBOAuthRedirectUri) -> Self {
        Self {
            id: value.id.into(),
            client_id: value.client_id.into(),
            uri: value.uri,
        }
    }
}

impl From<DBOAuthClientAuthorization> for OAuthClientAuthorization {
    fn from(value: DBOAuthClientAuthorization) -> Self {
        Self {
            id: value.id.into(),
            app_id: value.client_id.into(),
            user_id: value.user_id.into(),
            scopes: value.scopes,
            created: value.created,
        }
    }
}
