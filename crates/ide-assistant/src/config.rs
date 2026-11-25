//! The setup on disk, `assistant.json` beside the settings. No secret is in it.

use std::fs;
use std::io;
use std::path::Path;

use ide_core::IdeError;
use ide_core::assistant::{BedrockAuth, ProviderKind, ProviderSetup};
use serde::{Deserialize, Serialize};

pub(crate) const FILE: &str = "assistant.json";
const VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SetupFile {
    version: Option<u32>,
    provider: Option<String>,
    model: Option<String>,
    endpoint: Option<String>,
    region: Option<String>,
    profile: Option<String>,
    /// `profile` or `apiKey`.
    bedrock_auth: Option<String>,
}

fn kind(id: &str) -> Option<ProviderKind> {
    ProviderKind::ALL.into_iter().find(|kind| kind.id() == id)
}

/// The saved setup; `None` when there's none, or it isn't usable.
pub(crate) fn read(dir: &Path) -> Result<Option<ProviderSetup>, IdeError> {
    let bytes = match fs::read(dir.join(FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(IdeError::from_io(&error)),
    };
    let Ok(file) = serde_json::from_slice::<SetupFile>(&bytes) else {
        return Ok(None);
    };
    let (Some(kind), Some(model)) = (file.provider.as_deref().and_then(kind), file.model) else {
        return Ok(None);
    };
    Ok(Some(ProviderSetup {
        kind,
        model,
        endpoint: file.endpoint,
        region: file.region,
        profile: file.profile,
        bedrock_auth: match file.bedrock_auth.as_deref() {
            Some("apiKey") => BedrockAuth::ApiKey,
            _ => BedrockAuth::Profile,
        },
    }))
}

pub(crate) fn write(dir: &Path, setup: &ProviderSetup) -> Result<(), IdeError> {
    let file = SetupFile {
        version: Some(VERSION),
        provider: Some(setup.kind.id().to_owned()),
        model: Some(setup.model.clone()),
        endpoint: setup.endpoint.clone(),
        region: setup.region.clone(),
        profile: setup.profile.clone(),
        bedrock_auth: Some(
            match setup.bedrock_auth {
                BedrockAuth::Profile => "profile",
                BedrockAuth::ApiKey => "apiKey",
            }
            .to_owned(),
        ),
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|error| IdeError::Io(error.to_string()))?;
    fs::create_dir_all(dir).map_err(|error| IdeError::from_io(&error))?;
    // Written beside it, then moved over it: never half a file.
    let tmp = dir.join(format!("{FILE}.tmp"));
    fs::write(&tmp, json).map_err(|error| IdeError::from_io(&error))?;
    fs::rename(&tmp, dir.join(FILE)).map_err(|error| IdeError::from_io(&error))
}

pub(crate) fn remove(dir: &Path) -> Result<(), IdeError> {
    match fs::remove_file(dir.join(FILE)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(IdeError::from_io(&error)),
    }
}

/// What's wrong with `setup`, if anything.
pub(crate) fn validate(setup: &ProviderSetup) -> Result<(), IdeError> {
    let invalid = |message: &str| Err(IdeError::InvalidArgument(message.to_owned()));
    if setup.model.trim().is_empty() {
        return invalid(if setup.kind == ProviderKind::AzureFoundry {
            "enter the deployment's name"
        } else {
            "choose a model"
        });
    }
    match setup.kind {
        ProviderKind::AzureFoundry => {
            let endpoint = setup.endpoint.as_deref().unwrap_or_default().trim();
            if !endpoint.starts_with("https://") {
                return invalid("enter the resource's endpoint, starting with https://");
            }
        }
        ProviderKind::Ollama => {
            if let Some(endpoint) = setup.endpoint.as_deref().map(str::trim)
                && !endpoint.is_empty()
                && !endpoint.starts_with("http://")
                && !endpoint.starts_with("https://")
            {
                return invalid("the Ollama address must start with http:// or https://");
            }
        }
        ProviderKind::Bedrock => {
            let region = setup.region.as_deref().unwrap_or_default().trim();
            if !region.is_empty()
                && !region
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                return invalid("the region is written like us-east-1");
            }
        }
        _ => {}
    }
    Ok(())
}
