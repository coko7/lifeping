//! Site title and UI strings, rendered into `index.html` once at startup.

use std::collections::BTreeMap;

use serde::Serialize;

const INDEX_TEMPLATE: &str = include_str!("../web/index.html");
const DEFAULT_STRINGS: &str = include_str!("../web/strings.json");

pub const DEFAULT_TITLE: &str = "Life Ping";

/// Language code -> string key -> text.
pub type Strings = BTreeMap<String, BTreeMap<String, String>>;

/// The built-in strings from `web/strings.json`.
pub fn default_strings() -> Strings {
    serde_json::from_str(DEFAULT_STRINGS).expect("web/strings.json is valid")
}

/// Merges `overrides` (JSON of the same shape as `web/strings.json`, any subset
/// of it) over the defaults. Unknown languages or keys are rejected so typos
/// don't go unnoticed.
pub fn merge_overrides(overrides: &str) -> Result<Strings, String> {
    let overrides: Strings = serde_json::from_str(overrides).map_err(|e| {
        format!("invalid JSON ({e}); expected {{\"en\": {{\"key\": \"text\"}}, …}}")
    })?;
    let mut strings = default_strings();
    for (lang, entries) in overrides {
        let Some(target) = strings.get_mut(&lang) else {
            let known: Vec<_> = default_strings().into_keys().collect();
            return Err(format!(
                "unknown language {lang:?}; expected one of {known:?}"
            ));
        };
        for (key, text) in entries {
            let Some(slot) = target.get_mut(&key) else {
                let known: Vec<_> = target.keys().cloned().collect();
                return Err(format!(
                    "unknown key {key:?} for {lang:?}; expected one of {known:?}"
                ));
            };
            *slot = text;
        }
    }
    Ok(strings)
}

#[derive(Serialize)]
struct SiteConfig<'a> {
    title: &'a str,
    strings: &'a Strings,
}

/// Fills the `{{title}}` and `{{config}}` placeholders of `web/index.html`.
pub fn render_index(title: &str, strings: &Strings) -> String {
    let config = serde_json::to_string(&SiteConfig { title, strings })
        .expect("site config serializes")
        // Keeps `</script>` and friends from ending the JSON block early.
        .replace('<', "\\u003c");
    INDEX_TEMPLATE
        .replace("{{title}}", &escape_html(title))
        .replace("{{config}}", &config)
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_the_same_keys_in_every_language() {
        let strings = default_strings();
        let en: Vec<_> = strings["en"].keys().collect();
        for (lang, entries) in &strings {
            assert_eq!(entries.keys().collect::<Vec<_>>(), en, "{lang}");
        }
    }

    #[test]
    fn partial_override_keeps_other_defaults() {
        let strings = merge_overrides(r#"{"fr": {"headline.green": "Toujours là"}}"#).unwrap();
        assert_eq!(strings["fr"]["headline.green"], "Toujours là");
        assert_eq!(
            strings["fr"]["headline.unknown"],
            "Aucun ping pour l'instant"
        );
        assert_eq!(strings["en"], default_strings()["en"]);
    }

    #[test]
    fn rejects_unknown_language_key_and_bad_json() {
        for bad in [
            r#"{"de": {"headline.green": "Lebt"}}"#,
            r#"{"en": {"headline.gren": "Alive"}}"#,
            r#"{"en": {"headline.green": 1}}"#,
            "not json",
        ] {
            assert!(merge_overrides(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn render_escapes_title_and_config() {
        let html = render_index("<b>Tom & \"Jerry\"</b>", &default_strings());
        assert!(html.contains("&lt;b&gt;Tom &amp; &quot;Jerry&quot;&lt;/b&gt;"));
        assert!(!html.contains("<b>"));
        assert!(!html.contains("{{"));
        assert!(html.contains(r#""title":"\u003cb>Tom & \"Jerry\"\u003c/b>""#));
    }
}
