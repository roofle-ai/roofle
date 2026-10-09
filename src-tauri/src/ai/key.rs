/// Local/dev key sources, checked in order; they override the renderer's key.
const KEY_ENV_VARS: &[&str] = &["ROOFLE_LOCAL_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"];

pub fn env_api_key() -> Option<String> {
    KEY_ENV_VARS.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    })
}

/// A local env key wins over the renderer-supplied one. Never persisted.
pub fn resolve_api_key(renderer_key: &str) -> String {
    env_api_key().unwrap_or_else(|| renderer_key.trim().to_string())
}

/// `app_config` key holding the API key typed in Settings.
pub const KEY_SETTING: &str = "ai_api_key";

/// Env key, else the renderer's key, else the one saved in Settings.
pub fn resolve_api_key_with(storage: &roofle_core::storage::SqliteStorage, renderer_key: &str) -> String {
    let resolved = resolve_api_key(renderer_key);
    if !resolved.is_empty() {
        return resolved;
    }
    storage.get_config(KEY_SETTING).ok().flatten().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests mutate the process env; serialize them to avoid races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_env() {
        for name in KEY_ENV_VARS {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn env_key_falls_back_to_provider_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();

        std::env::set_var("OPENROUTER_API_KEY", "sk-openrouter");
        assert_eq!(resolve_api_key(""), "sk-openrouter");

        std::env::set_var("ROOFLE_LOCAL_KEY", "sk-local");
        assert_eq!(resolve_api_key(""), "sk-local");
    }

    #[test]
    fn renderer_key_used_when_no_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        clear_env();

        assert_eq!(resolve_api_key("  sk-renderer  "), "sk-renderer");
    }
}
