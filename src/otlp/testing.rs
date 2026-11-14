//! Credentials that serve scripted access tokens, for tests that need Google auth headers
//! without a token endpoint.

use gcloud_sdk::google_cloud_auth::credentials::{
    CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use gcloud_sdk::google_cloud_auth::errors::CredentialsError;
use gcloud_sdk::tonic::codegen::http::header::AUTHORIZATION;
use gcloud_sdk::tonic::codegen::http::{Extensions, HeaderMap, HeaderValue};
use gcloud_sdk::GoogleAuthHeaders;
use std::sync::{Arc, Mutex};

/// Serves the access token last scripted under an entity tag that changes with the token,
/// the way real credentials answer `NotModified` until they hold a new one. Fails while no
/// token is scripted. Clones share the script.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScriptedAccessToken {
    current: Arc<Mutex<Option<(EntityTag, String)>>>,
}

impl ScriptedAccessToken {
    pub(crate) fn new(token: &str) -> Self {
        let credentials = Self::default();
        credentials.rotate(token);
        credentials
    }

    pub(crate) fn rotate(&self, token: &str) {
        *self.current.lock().unwrap() = Some((EntityTag::new(), token.to_string()));
    }

    pub(crate) fn auth_headers(&self) -> Arc<GoogleAuthHeaders> {
        Arc::new(GoogleAuthHeaders::from(Credentials::from(self.clone())))
    }
}

impl CredentialsProvider for ScriptedAccessToken {
    async fn headers(
        &self,
        extensions: Extensions,
    ) -> Result<CacheableResource<HeaderMap>, CredentialsError> {
        let (entity_tag, token) = self
            .current
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| CredentialsError::from_msg(false, "no access token is scripted"))?;
        if extensions.get::<EntityTag>() == Some(&entity_tag) {
            return Ok(CacheableResource::NotModified);
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        Ok(CacheableResource::New {
            entity_tag,
            data: headers,
        })
    }

    async fn universe_domain(&self) -> Option<String> {
        None
    }
}
