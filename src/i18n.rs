//! Строки интерфейса: en/ru из системного окружения.
//!
//! Словари вкомпилированы в бинарник: каталога локалей на диске нет.

use std::collections::HashMap;
use std::sync::OnceLock;

const EN_JSON: &str = include_str!("locales/en.json");
const RU_JSON: &str = include_str!("locales/ru.json");

/// Язык, выбранный при первом обращении, то есть при запуске.
/// Смена языка — только перезапуском: окружение читается один раз.
static LANG: OnceLock<&'static str> = OnceLock::new();

fn parse(raw: &str) -> HashMap<String, String> {
    serde_json::from_str(raw).expect("словарь i18n битый")
}

fn english() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| parse(EN_JSON))
}

fn current() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| parse(if lang() == "ru" { RU_JSON } else { EN_JSON }))
}

/// Язык системы: `LANGUAGE` → `LC_ALL` → `LC_MESSAGES` → `LANG`.
pub fn lang() -> &'static str {
    LANG.get_or_init(|| {
        let var = |name: &str| std::env::var(name).ok();
        let language = var("LANGUAGE");
        let all = var("LC_ALL");
        let messages = var("LC_MESSAGES");
        let fallback = var("LANG");
        pick(
            language.as_deref(),
            all.as_deref(),
            messages.as_deref(),
            fallback.as_deref(),
        )
    })
}

/// Чистый выбор языка: принимает значения переменных, а не читает окружение.
/// Отрезает `.UTF-8` и `@модификатор`; всё незнакомое — английский.
pub fn pick(
    language: Option<&str>,
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> &'static str {
    fn chosen(value: Option<&str>) -> Option<&str> {
        value?
            .split(':')
            .map(str::trim)
            .find(|part| !part.is_empty())
    }
    let Some(raw) = chosen(language)
        .or_else(|| chosen(lc_all))
        .or_else(|| chosen(lc_messages))
        .or_else(|| chosen(lang))
    else {
        return "en";
    };
    let raw = raw.split('@').next().unwrap_or(raw);
    let raw = raw.split('.').next().unwrap_or(raw);
    let code = raw.split('_').next().unwrap_or(raw).to_ascii_lowercase();
    if code == "ru" { "ru" } else { "en" }
}

/// Перевод по ключу. Неизвестный ключ возвращает сам ключ: так его видно.
pub fn t(key: &str) -> &str {
    current()
        .get(key)
        .or_else(|| english().get(key))
        .map(String::as_str)
        .unwrap_or(key)
}

/// Перевод с именованными плейсхолдерами `{name}`.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = t(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// Словарь и `lang` для страницы. Вставляется перед бандлом редактора,
/// поэтому placeholder и панель поиска видят перевод при создании EditorView.
pub fn js() -> String {
    let json = if lang() == "ru" { RU_JSON } else { EN_JSON };
    format!(
        "window.mcI18n={};document.documentElement.lang=\"{}\";",
        json.replace('<', "\\u003c"),
        lang()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn keys(raw: &str) -> BTreeSet<String> {
        parse(raw).into_keys().collect()
    }

    #[test]
    fn dictionaries_have_same_keys() {
        assert_eq!(keys(EN_JSON), keys(RU_JSON));
    }

    #[test]
    fn language_follows_environment() {
        assert_eq!(pick(None, None, None, Some("ru_RU.UTF-8")), "ru");
        assert_eq!(pick(None, None, None, Some("en_US.UTF-8")), "en");
        assert_eq!(pick(None, None, None, Some("C")), "en");
        assert_eq!(pick(Some("ru:en"), None, None, Some("en_US.UTF-8")), "ru");
        assert_eq!(pick(None, None, None, None), "en");
    }

    #[test]
    fn tf_replaces_placeholder() {
        let text = tf("error.save", &[("error", "boom")]);
        assert!(text.contains("boom"), "{text}");
        assert!(!text.contains("{error}"), "{text}");
    }
}
