//! The providers' API keys, kept in the system's credential store (Windows
//! Credential Manager, the macOS Keychain, the Secret Service on Linux),
//! never in a file of the app's. One per provider.

use ide_core::assistant::ProviderKind;

/// Where the keys are kept.
pub(crate) trait Secrets: Send + Sync {
    fn get(&self, provider: ProviderKind) -> Result<Option<String>, String>;
    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), String>;
    /// Removes it; nothing happens when there's none.
    fn delete(&self, provider: ProviderKind) -> Result<(), String>;
}

/// The name the keys are kept under in the store.
const SERVICE: &str = "webide-assistant";

/// The system's credential store.
pub(crate) struct Keyring;

fn entry(provider: ProviderKind) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, provider.id())
        .map_err(|error| format!("the system's credential store can't be used: {error}"))
}

impl Secrets for Keyring {
    fn get(&self, provider: ProviderKind) -> Result<Option<String>, String> {
        match entry(provider)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!(
                "the key couldn't be read from the credential store: {error}"
            )),
        }
    }

    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), String> {
        entry(provider)?
            .set_password(secret)
            .map_err(|error| format!("the key couldn't be saved in the credential store: {error}"))
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), String> {
        match entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!(
                "the key couldn't be removed from the credential store: {error}"
            )),
        }
    }
}

/// Keys in memory, for tests.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct InMemory {
    keys: std::sync::Mutex<std::collections::HashMap<ProviderKind, String>>,
}

#[cfg(test)]
impl Secrets for InMemory {
    fn get(&self, provider: ProviderKind) -> Result<Option<String>, String> {
        Ok(self
            .keys
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .get(&provider)
            .cloned())
    }

    fn set(&self, provider: ProviderKind, secret: &str) -> Result<(), String> {
        self.keys
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .insert(provider, secret.to_owned());
        Ok(())
    }

    fn delete(&self, provider: ProviderKind) -> Result<(), String> {
        self.keys
            .lock()
            .map_err(|_| "poisoned".to_owned())?
            .remove(&provider);
        Ok(())
    }
}
