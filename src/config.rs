//! Configuration from environment variables, validated at startup.

use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

use time::Duration;

use crate::site::{self, Strings};

pub const DEFAULT_BIND: &str = "0.0.0.0:8080";

const MIN_RECOMMENDED_TOKEN_LEN: usize = 32;
const MAX_HISTORY: usize = 1000;
const MAX_TITLE_LEN: usize = 100;

#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub yellow_after: Duration,
    pub red_after: Duration,
    pub history: usize,
    pub data_dir: PathBuf,
    pub bind: SocketAddr,
    pub title: String,
    pub strings: Strings,
}

// Hand-written so the token can never end up in logs through `{:?}`.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("token", &"<redacted>")
            .field("yellow_after", &self.yellow_after)
            .field("red_after", &self.red_after)
            .field("history", &self.history)
            .field("data_dir", &self.data_dir)
            .field("bind", &self.bind)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub var: &'static str,
    pub message: String,
}

impl ConfigError {
    fn new(var: &'static str, message: impl Into<String>) -> Self {
        Self {
            var,
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.var, self.message)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Builds the configuration from an arbitrary variable source, so tests
    /// don't have to mutate the process environment.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let token = parse_token(&get)?;
        if token.chars().count() < MIN_RECOMMENDED_TOKEN_LEN {
            tracing::warn!(
                "the configured token is shorter than {MIN_RECOMMENDED_TOKEN_LEN} characters; \
                 consider generating one with `openssl rand -hex 32`"
            );
        }

        let yellow_after = parse_duration(&get, "LIFEPING_YELLOW_AFTER", "12h")?;
        let red_after = parse_duration(&get, "LIFEPING_RED_AFTER", "24h")?;
        if red_after <= yellow_after {
            return Err(ConfigError::new(
                "LIFEPING_RED_AFTER",
                "must be strictly greater than LIFEPING_YELLOW_AFTER",
            ));
        }

        let history = match get("LIFEPING_HISTORY") {
            None => 10,
            Some(raw) => match raw.trim().parse::<usize>() {
                Ok(n) if (1..=MAX_HISTORY).contains(&n) => n,
                _ => {
                    return Err(ConfigError::new(
                        "LIFEPING_HISTORY",
                        format!("must be an integer between 1 and {MAX_HISTORY}, got {raw:?}"),
                    ));
                }
            },
        };

        let data_dir = PathBuf::from(get("LIFEPING_DATA_DIR").unwrap_or_else(|| "/data".into()));
        let bind = parse_bind(get("LIFEPING_BIND"))?;
        let title = parse_title(get("LIFEPING_TITLE"))?;
        let strings = parse_strings(get("LIFEPING_STRINGS_FILE"))?;

        Ok(Self {
            token,
            yellow_after,
            red_after,
            history,
            data_dir,
            bind,
            title,
            strings,
        })
    }
}

/// Parses `LIFEPING_BIND` on its own; the healthcheck needs nothing else.
pub fn parse_bind(raw: Option<String>) -> Result<SocketAddr, ConfigError> {
    let raw = raw.unwrap_or_else(|| DEFAULT_BIND.into());
    raw.trim().parse().map_err(|_| {
        ConfigError::new(
            "LIFEPING_BIND",
            format!("must be a socket address like {DEFAULT_BIND}, got {raw:?}"),
        )
    })
}

fn parse_token(get: &impl Fn(&str) -> Option<String>) -> Result<String, ConfigError> {
    let token = match (get("LIFEPING_TOKEN"), get("LIFEPING_TOKEN_FILE")) {
        (Some(_), Some(_)) => {
            return Err(ConfigError::new(
                "LIFEPING_TOKEN",
                "set either LIFEPING_TOKEN or LIFEPING_TOKEN_FILE, not both",
            ));
        }
        (None, None) => {
            return Err(ConfigError::new(
                "LIFEPING_TOKEN",
                "one of LIFEPING_TOKEN or LIFEPING_TOKEN_FILE must be set",
            ));
        }
        (Some(token), None) => (token, "LIFEPING_TOKEN"),
        (None, Some(path)) => {
            let contents = std::fs::read_to_string(&path).map_err(|e| {
                ConfigError::new(
                    "LIFEPING_TOKEN_FILE",
                    format!("cannot read token file {path:?}: {e}"),
                )
            })?;
            (contents.trim_end().to_owned(), "LIFEPING_TOKEN_FILE")
        }
    };
    match token {
        (token, var) if token.is_empty() => {
            Err(ConfigError::new(var, "the token must not be empty"))
        }
        (token, _) => Ok(token),
    }
}

fn parse_title(raw: Option<String>) -> Result<String, ConfigError> {
    let Some(raw) = raw else {
        return Ok(site::DEFAULT_TITLE.into());
    };
    let title = raw.trim();
    if title.is_empty() {
        return Err(ConfigError::new("LIFEPING_TITLE", "must not be empty"));
    }
    if title.chars().count() > MAX_TITLE_LEN {
        return Err(ConfigError::new(
            "LIFEPING_TITLE",
            format!("must be at most {MAX_TITLE_LEN} characters"),
        ));
    }
    Ok(title.into())
}

fn parse_strings(path: Option<String>) -> Result<Strings, ConfigError> {
    let Some(path) = path else {
        return Ok(site::default_strings());
    };
    let contents = std::fs::read_to_string(&path).map_err(|e| {
        ConfigError::new(
            "LIFEPING_STRINGS_FILE",
            format!("cannot read strings file {path:?}: {e}"),
        )
    })?;
    site::merge_overrides(&contents)
        .map_err(|e| ConfigError::new("LIFEPING_STRINGS_FILE", format!("{path:?}: {e}")))
}

fn parse_duration(
    get: &impl Fn(&str) -> Option<String>,
    var: &'static str,
    default: &str,
) -> Result<Duration, ConfigError> {
    let raw = get(var).unwrap_or_else(|| default.into());
    let parsed = humantime::parse_duration(raw.trim()).map_err(|e| {
        ConfigError::new(
            var,
            format!("invalid duration {raw:?} ({e}); use e.g. 12h or 1d 6h"),
        )
    })?;
    let duration = Duration::try_from(parsed)
        .map_err(|_| ConfigError::new(var, format!("duration {raw:?} is too large")))?;
    if duration <= Duration::ZERO {
        return Err(ConfigError::new(var, "must be greater than zero"));
    }
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;

    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn load(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|key| map.get(key).cloned())
    }

    fn err_var(vars: &[(&str, &str)]) -> &'static str {
        load(vars).expect_err("config should be rejected").var
    }

    #[test]
    fn defaults() {
        let config = load(&[("LIFEPING_TOKEN", TOKEN)]).unwrap();
        assert_eq!(config.token, TOKEN);
        assert_eq!(config.yellow_after, Duration::hours(12));
        assert_eq!(config.red_after, Duration::hours(24));
        assert_eq!(config.history, 10);
        assert_eq!(config.data_dir, PathBuf::from("/data"));
        assert_eq!(config.bind, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(config.title, "Life Ping");
        assert_eq!(config.strings, site::default_strings());
    }

    #[test]
    fn custom_values() {
        let config = load(&[
            ("LIFEPING_TOKEN", TOKEN),
            ("LIFEPING_YELLOW_AFTER", "1d 6h"),
            ("LIFEPING_RED_AFTER", "2d"),
            ("LIFEPING_HISTORY", "1000"),
            ("LIFEPING_DATA_DIR", "/tmp/lp"),
            ("LIFEPING_BIND", "127.0.0.1:9000"),
        ])
        .unwrap();
        assert_eq!(config.yellow_after, Duration::hours(30));
        assert_eq!(config.red_after, Duration::days(2));
        assert_eq!(config.history, 1000);
        assert_eq!(config.data_dir, PathBuf::from("/tmp/lp"));
        assert_eq!(config.bind, "127.0.0.1:9000".parse().unwrap());
    }

    #[test]
    fn custom_title_is_trimmed() {
        let config = load(&[
            ("LIFEPING_TOKEN", TOKEN),
            ("LIFEPING_TITLE", "  Coko's pulse "),
        ])
        .unwrap();
        assert_eq!(config.title, "Coko's pulse");
    }

    #[test]
    fn bad_titles() {
        let long = "x".repeat(MAX_TITLE_LEN + 1);
        for bad in ["", "   ", &long] {
            let vars = [("LIFEPING_TOKEN", TOKEN), ("LIFEPING_TITLE", bad)];
            assert_eq!(err_var(&vars), "LIFEPING_TITLE", "value {bad:?}");
        }
    }

    #[test]
    fn strings_file_overrides_defaults() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, r#"{{"en": {{"headline.green": "Still kicking"}}}}"#).unwrap();
        let path = file.path().to_str().unwrap();
        let config = load(&[("LIFEPING_TOKEN", TOKEN), ("LIFEPING_STRINGS_FILE", path)]).unwrap();
        assert_eq!(config.strings["en"]["headline.green"], "Still kicking");
        assert_eq!(config.strings["fr"], site::default_strings()["fr"]);
    }

    #[test]
    fn bad_strings_files() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, r#"{{"en": {{"no.such.key": "x"}}}}"#).unwrap();
        let invalid = file.path().to_str().unwrap();
        for path in [invalid, "/nonexistent/lifeping/strings.json"] {
            let vars = [("LIFEPING_TOKEN", TOKEN), ("LIFEPING_STRINGS_FILE", path)];
            assert_eq!(err_var(&vars), "LIFEPING_STRINGS_FILE", "path {path:?}");
        }
    }

    #[test]
    fn missing_token() {
        assert_eq!(err_var(&[]), "LIFEPING_TOKEN");
    }

    #[test]
    fn empty_token() {
        assert_eq!(err_var(&[("LIFEPING_TOKEN", "")]), "LIFEPING_TOKEN");
    }

    #[test]
    fn both_token_vars() {
        assert_eq!(
            err_var(&[("LIFEPING_TOKEN", TOKEN), ("LIFEPING_TOKEN_FILE", "/x")]),
            "LIFEPING_TOKEN"
        );
    }

    #[test]
    fn token_file_is_trimmed() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{TOKEN}  ").unwrap();
        let path = file.path().to_str().unwrap();
        let config = load(&[("LIFEPING_TOKEN_FILE", path)]).unwrap();
        assert_eq!(config.token, TOKEN);
    }

    #[test]
    fn blank_token_file() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "   ").unwrap();
        let path = file.path().to_str().unwrap();
        assert_eq!(
            err_var(&[("LIFEPING_TOKEN_FILE", path)]),
            "LIFEPING_TOKEN_FILE"
        );
    }

    #[test]
    fn unreadable_token_file() {
        assert_eq!(
            err_var(&[("LIFEPING_TOKEN_FILE", "/nonexistent/lifeping/token")]),
            "LIFEPING_TOKEN_FILE"
        );
    }

    #[test]
    fn red_not_after_yellow() {
        let base = [("LIFEPING_TOKEN", TOKEN), ("LIFEPING_YELLOW_AFTER", "12h")];
        for red in ["12h", "6h"] {
            let vars = [base[0], base[1], ("LIFEPING_RED_AFTER", red)];
            assert_eq!(err_var(&vars), "LIFEPING_RED_AFTER");
        }
    }

    #[test]
    fn bad_durations() {
        for bad in ["", "soon", "12", "-1h", "0s"] {
            let vars = [("LIFEPING_TOKEN", TOKEN), ("LIFEPING_YELLOW_AFTER", bad)];
            assert_eq!(err_var(&vars), "LIFEPING_YELLOW_AFTER", "value {bad:?}");
        }
    }

    #[test]
    fn history_out_of_range() {
        for bad in ["0", "1001", "-3", "ten", ""] {
            let vars = [("LIFEPING_TOKEN", TOKEN), ("LIFEPING_HISTORY", bad)];
            assert_eq!(err_var(&vars), "LIFEPING_HISTORY", "value {bad:?}");
        }
    }

    #[test]
    fn bad_bind() {
        let vars = [("LIFEPING_TOKEN", TOKEN), ("LIFEPING_BIND", "localhost")];
        assert_eq!(err_var(&vars), "LIFEPING_BIND");
    }

    #[test]
    fn debug_redacts_token() {
        let config = load(&[("LIFEPING_TOKEN", TOKEN)]).unwrap();
        assert!(!format!("{config:?}").contains(TOKEN));
    }
}
