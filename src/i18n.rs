use crate::domain::shared::game::TB;
use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{Loader, static_loader};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{OnceLock,RwLock,RwLockReadGuard};
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

static INSTANCE: OnceLock<RwLock<I18nManager>> = OnceLock::new();
impl I18nManager {
    pub fn init(lang_str: &str) {
        let normalized=lang_str.to_ascii_lowercase().replace('_',"-");
        let lang=if normalized=="jp" || normalized.starts_with("ja") || normalized=="jp-jn" {langid!("ja-JP")} else {langid!("en-US")};
        let instance=INSTANCE.get_or_init(||RwLock::new(I18nManager {lang:langid!("en-US")}));
        instance.write().unwrap_or_else(|p|p.into_inner()).lang=lang;
    }

    pub fn global() -> RwLockReadGuard<'static,I18nManager> {
        INSTANCE.get_or_init(||RwLock::new(I18nManager {lang:langid!("en-US")}))
            .read().unwrap_or_else(|p|p.into_inner())
    }

    pub fn is_japanese(&self)->bool { self.lang.language.as_str()=="ja" }

    pub fn tr(&self, key: &str) -> String {
        LOCALES.lookup(&self.lang, key)
    }

    pub fn tr_with(&self, key: &str, args: HashMap<Cow<'static, str>, FluentValue>) -> String {
        LOCALES.lookup_with_args(&self.lang, key, &args)
    }

    pub fn lang_db(&self) -> String {
        let mut lang_db = "us".to_string();
        if self.is_japanese() {
            lang_db = "jp".to_string();
        }
        lang_db
    }

    pub fn full_name(&self, first_name: &str, last_name: &str) -> String {
        let mut full_name = format!("{} {}", first_name, last_name);
        if self.is_japanese() {
            full_name = format!("{} {}", last_name, first_name);
        }
        full_name
    }

    pub fn inning(&self, inning_seq: u8, inning_tb: TB) -> String {
        if self.is_japanese() {
            let tb = match inning_tb {
                TB::Top => "表",
                TB::Bottom => "裏",
            };
            return format!("{inning_seq}回{tb}");
        }

        let tb = match inning_tb {
            TB::Top => "top",
            TB::Bottom => "bottom",
        };
        format!("the {tb} of the {}", Self::ordinal(u16::from(inning_seq)))
    }

    pub fn homerun(&self, home_runs: u16) -> String {
        if self.is_japanese() {
            return format!("{home_runs}号");
        }

        Self::ordinal(home_runs)
    }

    fn ordinal(number: u16) -> String {
        let suffix = match number % 100 {
            11..=13 => "th",
            _ => match number % 10 {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            },
        };
        format!("{number}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_name_order_follows_language_including_short_japanese_tag() {
        for lang in [langid!("ja-JP"),langid!("ja")] {
            let manager=I18nManager {lang};
            assert_eq!(manager.full_name("翔平","大谷"),"大谷 翔平");
        }
        assert_eq!(I18nManager {lang:langid!("en-US")}.full_name("John","Smith"),"John Smith");
    }

    #[test]
    fn inning_formats_japanese_half_inning() {
        let manager = I18nManager {
            lang: langid!("ja-JP"),
        };

        assert_eq!(manager.inning(1, TB::Top), "1回表");
        assert_eq!(manager.inning(9, TB::Bottom), "9回裏");
    }

    #[test]
    fn inning_formats_english_half_inning_with_ordinals() {
        let manager = I18nManager {
            lang: langid!("en-US"),
        };

        assert_eq!(manager.inning(1, TB::Top), "the top of the 1st");
        assert_eq!(manager.inning(2, TB::Bottom), "the bottom of the 2nd");
        assert_eq!(manager.inning(3, TB::Top), "the top of the 3rd");
        assert_eq!(manager.inning(11, TB::Bottom), "the bottom of the 11th");
    }

    #[test]
    fn homerun_formats_japanese_season_number() {
        let manager = I18nManager {
            lang: langid!("ja-JP"),
        };

        assert_eq!(manager.homerun(2), "2号");
    }

    #[test]
    fn homerun_formats_english_season_number_with_ordinals() {
        let manager = I18nManager {
            lang: langid!("en-US"),
        };

        assert_eq!(manager.homerun(1), "1st");
        assert_eq!(manager.homerun(2), "2nd");
        assert_eq!(manager.homerun(3), "3rd");
        assert_eq!(manager.homerun(11), "11th");
        assert_eq!(manager.homerun(22), "22nd");
    }
}
