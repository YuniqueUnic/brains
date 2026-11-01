//! Credential changes never return or log the replacement token.

use crate::{BrainClient, ClientResult};

impl BrainClient {
    pub fn rotate_token(&self, replacement_token: &str) -> ClientResult<()> {
        let response = self.send(self.http.post(self.url("/api/v1/token/rotate", "")).json(
            &brains_sdk::TokenRotationInput {
                current_token: self.token.clone(),
                replacement_token: replacement_token.to_owned(),
            },
        ))?;
        self.expect_json::<brains_sdk::OkResult>(response)?;
        Ok(())
    }

    pub fn revoke_sessions(&self) -> ClientResult<()> {
        let response = self.send(self.http.post(self.url("/api/v1/session/revoke", "")))?;
        self.expect_json::<brains_sdk::OkResult>(response)?;
        Ok(())
    }
}
