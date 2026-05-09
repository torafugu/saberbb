use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{Loader, static_loader};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;
use unic_langid::LanguageIdentifier;
use unic_langid::langid;

static_loader! {
    static LOCALES = {
        locales: "locales",
        fallback_language: "en-US",    };
}

#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::I18nManager::global().tr($key)
    };
    ($key:expr, $($name:expr => $value:expr),*) => {{
        let mut args = std::collections::HashMap::new();
        $(
            args.insert(std::borrow::Cow::from($name), fluent_templates::fluent_bundle::FluentValue::from($value));
        )*
        $crate::I18nManager::global().tr_with($key, args)
    }};
}

pub struct I18nManager {
    pub lang: LanguageIdentifier,
}

static INSTANCE: OnceLock<I18nManager> = OnceLock::new();
impl I18nManager {
    pub fn init(lang_str: &str) {
        let lang = lang_str.parse().unwrap_or_else(|_| langid!("en-US"));
        let manager = I18nManager { lang };
        let _ = INSTANCE.set(manager);
    }

    pub fn global() -> &'static I18nManager {
        INSTANCE.get_or_init(|| {
            let lang = langid!("en-US");
            I18nManager { lang }
        })
    }

    pub fn tr(&self, key: &str) -> String {
        LOCALES.lookup(&self.lang, key)
    }

    pub fn tr_with(&self, key: &str, args: HashMap<Cow<'static, str>, FluentValue>) -> String {
        LOCALES.lookup_with_args(&self.lang, key, &args)
    }

    pub fn lang_db(&self) -> String {
        let mut lang_db = "us".to_string();
        if self.lang.to_string() == "ja-JP" {
            lang_db = "jp".to_string();
        }
        lang_db
    }
}
