//! Explicit remote-profile credential management; stdin and private config only.

use crate::cli::AuthCommand;
use crate::config::{Config, Profile};
use brains_client::{BrainClient, ClientError, ClientResult};
use brains_domain::DomainError;
use std::io::Read;

pub(crate) fn run(
    config: &Config,
    selected: Option<&str>,
    command: AuthCommand,
) -> ClientResult<i32> {
    if std::env::var_os("BRAINS_URL").is_some() || std::env::var_os("BRAINS_TOKEN").is_some() {
        return Err(DomainError::InvalidArgument("unset BRAINS_URL/BRAINS_TOKEN and select a stored remote profile for credential changes".into()).into());
    }
    let name = selected.or(config.active.as_deref()).unwrap_or("default");
    let Profile::Remote { url, token } =
        config.resolve_named(name).map_err(ClientError::transport)?
    else {
        return Err(DomainError::InvalidArgument(
            "local profiles use filesystem permissions; select a remote server profile".into(),
        )
        .into());
    };
    let client = BrainClient::new(&url, &token);
    match command {
        AuthCommand::LogoutAll => {
            client.revoke_sessions()?;
            eprintln!("All browser sessions revoked. The profile token remains valid.");
        }
        AuthCommand::RotateToken { stdin_token: _ } => {
            let mut replacement = String::new();
            std::io::stdin()
                .take(4099)
                .read_to_string(&mut replacement)
                .map_err(|e| ClientError::transport(e.to_string()))?;
            let replacement = replacement
                .strip_suffix("\r\n")
                .or_else(|| replacement.strip_suffix('\n'))
                .unwrap_or(&replacement);
            if !(32..=4096).contains(&replacement.len())
                || !replacement.bytes().all(|b| (33..=126).contains(&b))
            {
                return Err(DomainError::InvalidArgument("replacement token must be 32..4096 printable ASCII characters without whitespace".into()).into());
            }
            client.rotate_token(replacement)?;
            // Re-read after the network operation, preserving unrelated edits and
            // refusing to replace a profile another process changed meanwhile.
            let save = (|| -> Result<(), String> {
                let mut latest = Config::load()?;
                match latest.profiles.get_mut(name) {
                    Some(Profile::Remote {
                        url: stored_url,
                        token: stored_token,
                    }) if *stored_url == url && *stored_token == token => {
                        *stored_token = replacement.to_owned()
                    }
                    _ => return Err("selected profile changed while rotation was in flight".into()),
                }
                latest.save()
            })();
            save.map_err(|e| ClientError::transport(format!("Token rotated remotely, but the profile could not be saved: {e}. Run bsx --profile {name} login --stdin-token with the replacement token.")))?;
            eprintln!(
                "Token rotated and selected profile updated. Previous token and browser sessions are invalid."
            );
        }
    }
    Ok(0)
}
