//! Multi-language (`en`, `ko`, `ja`, `zh`, `es`) and ANSI-colored `tracing` formatter.

use std::borrow::Cow;
use std::fmt;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicU8, Ordering};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime as LogSystemTime};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Supported log output languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogLang {
    En = 0,
    Ko = 1,
    Ja = 2,
    Zh = 3,
    Es = 4,
}

static ACTIVE_LANG: AtomicU8 = AtomicU8::new(LogLang::En as u8);

impl LogLang {
    /// Parses a language code (`"en"`, `"ko"`, `"ja"`, `"zh"`, `"es"`, or `"auto"`).
    pub fn parse(raw: &str) -> Option<Self> {
        let norm = raw.trim().to_ascii_lowercase();
        let code = norm.split(['_', '-', '.']).next().unwrap_or("");
        match code {
            "auto" => Some(Self::detect_from_env()),
            "en" | "english" => Some(Self::En),
            "ko" | "kr" | "korean" => Some(Self::Ko),
            "ja" | "jp" | "japanese" => Some(Self::Ja),
            "zh" | "cn" | "chinese" => Some(Self::Zh),
            "es" | "spanish" => Some(Self::Es),
            _ => None,
        }
    }

    /// Detects the preferred language from `FERRITE_LANG`, `LC_ALL`, or `LANG`.
    pub fn detect_from_env() -> Self {
        for key in ["FERRITE_LANG", "LC_ALL", "LANG"] {
            if let Ok(val) = std::env::var(key)
                && !val.trim().is_empty()
                && !val.eq_ignore_ascii_case("auto")
                && let Some(lang) = Self::parse(&val)
            {
                return lang;
            }
        }
        Self::En
    }

    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Ko,
            2 => Self::Ja,
            3 => Self::Zh,
            4 => Self::Es,
            _ => Self::En,
        }
    }
}

/// Sets the active runtime log language.
pub fn set_active_lang(lang: LogLang) {
    ACTIVE_LANG.store(lang as u8, Ordering::Relaxed);
}

/// Returns the active runtime log language.
pub fn active_lang() -> LogLang {
    LogLang::from_u8(ACTIVE_LANG.load(Ordering::Relaxed))
}

/// Returns whether ANSI color escapes should be emitted to stdout.
pub fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some() || std::env::var_os("FORCE_COLOR").is_some() {
        return true;
    }
    std::io::stdout().is_terminal()
}

/// Translates a log message into `lang`, falling back to the original English string.
pub fn translate_message<'a>(lang: LogLang, msg: &'a str) -> Cow<'a, str> {
    if lang == LogLang::En {
        return Cow::Borrowed(msg);
    }
    let translated = match (lang, msg) {
        // Korean
        (LogLang::Ko, "created the default ferrite.toml") => {
            Some("기본 ferrite.toml 설정 파일을 생성했습니다")
        }
        (LogLang::Ko, "loaded server configuration") => Some("서버 설정을 불러왔습니다"),
        (LogLang::Ko, "created missing startup directories") => {
            Some("누락된 시작 디렉터리를 생성했습니다")
        }
        (LogLang::Ko, "server icon not found; continuing without a favicon") => {
            Some("서버 아이콘을 찾을 수 없어 파비콘 없이 계속합니다")
        }
        (LogLang::Ko, "Ferrite listener started") => Some("Ferrite 서버 리스너가 시작되었습니다"),
        (LogLang::Ko, "using pure-Rust parallel world generator") => {
            Some("순수 Rust 병렬 월드 생성기를 사용합니다")
        }
        (LogLang::Ko, "loaded crafting recipes") => Some("제작 레시피를 불러왔습니다"),
        (LogLang::Ko, "entering play") => Some("플레이 단계에 진입했습니다"),
        (LogLang::Ko, "player joined") => Some("플레이어가 접속했습니다"),
        (LogLang::Ko, "disconnecting player") => Some("플레이어 연결을 종료합니다"),
        (LogLang::Ko, "connection closed") => Some("연결이 종료되었습니다"),
        (LogLang::Ko, "shutting down") => Some("서버를 종료합니다"),
        (LogLang::Ko, "saved world") => Some("월드를 저장했습니다"),

        // Japanese
        (LogLang::Ja, "created the default ferrite.toml") => {
            Some("デフォルトの ferrite.toml を作成しました")
        }
        (LogLang::Ja, "loaded server configuration") => Some("サーバー設定を読み込みました"),
        (LogLang::Ja, "created missing startup directories") => {
            Some("不足している起動ディレクトリを作成しました")
        }
        (LogLang::Ja, "server icon not found; continuing without a favicon") => {
            Some("サーバーアイコンが見つからないためファビコンなしで続行します")
        }
        (LogLang::Ja, "Ferrite listener started") => Some("Ferrite リスナーを開始しました"),
        (LogLang::Ja, "using pure-Rust parallel world generator") => {
            Some("純粋 Rust 並列ワールド生成器を使用します")
        }
        (LogLang::Ja, "loaded crafting recipes") => Some("クラフトレシピを読み込みました"),
        (LogLang::Ja, "entering play") => Some("プレイ状態に移行しました"),
        (LogLang::Ja, "player joined") => Some("プレイヤーが参加しました"),
        (LogLang::Ja, "disconnecting player") => Some("プレイヤーを切断しています"),
        (LogLang::Ja, "connection closed") => Some("接続が終了しました"),
        (LogLang::Ja, "shutting down") => Some("サーバーをシャットダウンしています"),
        (LogLang::Ja, "saved world") => Some("ワールドを保存しました"),

        // Chinese (Simplified)
        (LogLang::Zh, "created the default ferrite.toml") => {
            Some("已创建默认 ferrite.toml 配置文件")
        }
        (LogLang::Zh, "loaded server configuration") => Some("已加载服务器配置"),
        (LogLang::Zh, "created missing startup directories") => Some("已创建缺失的启动目录"),
        (LogLang::Zh, "server icon not found; continuing without a favicon") => {
            Some("未找到服务器图标，将在没有图标的情况下继续")
        }
        (LogLang::Zh, "Ferrite listener started") => Some("Ferrite 监听器已启动"),
        (LogLang::Zh, "using pure-Rust parallel world generator") => {
            Some("正在使用纯 Rust 并行世界生成器")
        }
        (LogLang::Zh, "loaded crafting recipes") => Some("已加载合成配方"),
        (LogLang::Zh, "entering play") => Some("已进入游戏阶段"),
        (LogLang::Zh, "player joined") => Some("玩家已加入"),
        (LogLang::Zh, "disconnecting player") => Some("正在断开玩家连接"),
        (LogLang::Zh, "connection closed") => Some("连接已关闭"),
        (LogLang::Zh, "shutting down") => Some("正在关闭服务器"),
        (LogLang::Zh, "saved world") => Some("已保存世界"),

        // Spanish
        (LogLang::Es, "created the default ferrite.toml") => {
            Some("se creó ferrite.toml por defecto")
        }
        (LogLang::Es, "loaded server configuration") => Some("configuración del servidor cargada"),
        (LogLang::Es, "created missing startup directories") => {
            Some("directorios de inicio creados")
        }
        (LogLang::Es, "server icon not found; continuing without a favicon") => {
            Some("no se encontró el icono del servidor; continuando sin favicon")
        }
        (LogLang::Es, "Ferrite listener started") => Some("listener de Ferrite iniciado"),
        (LogLang::Es, "using pure-Rust parallel world generator") => {
            Some("usando generador de mundo paralelo en Rust puro")
        }
        (LogLang::Es, "loaded crafting recipes") => Some("recetas de fabricación cargadas"),
        (LogLang::Es, "entering play") => Some("entrando en modo juego"),
        (LogLang::Es, "player joined") => Some("jugador conectado"),
        (LogLang::Es, "disconnecting player") => Some("desconectando al jugador"),
        (LogLang::Es, "connection closed") => Some("conexión cerrada"),
        (LogLang::Es, "shutting down") => Some("apagando el servidor"),
        (LogLang::Es, "saved world") => Some("mundo guardado"),
        _ => None,
    };
    match translated {
        Some(t) => Cow::Borrowed(t),
        None => Cow::Borrowed(msg),
    }
}

/// Custom `tracing` event formatter with ANSI color hierarchy and multi-language translation.
pub struct LocalizedColorFormatter {
    pub ansi: bool,
}

#[derive(Default)]
struct CollectedFields {
    message: String,
    pairs: Vec<(String, String)>,
}

impl Visit for CollectedFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.pairs.push((field.name().to_owned(), value.to_owned()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message.push_str(&format!("{value:?}"));
        } else {
            self.pairs
                .push((field.name().to_owned(), format!("{value:?}")));
        }
    }
}

impl<S, N> FormatEvent<S, N> for LocalizedColorFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let mut fields = CollectedFields::default();
        event.record(&mut fields);
        let level = *event.metadata().level();
        let mut time_buf = String::with_capacity(32);
        let _ = LogSystemTime.format_time(&mut Writer::new(&mut time_buf));
        let time = time_buf
            .split('T')
            .nth(1)
            .and_then(|t| t.get(..8))
            .unwrap_or(time_buf.as_str());
        let lang = active_lang();
        let msg = translate_message(lang, &fields.message);

        if self.ansi {
            let (lvl_color, msg_color) = match level {
                Level::ERROR => ("\x1b[1;31m", "\x1b[31m"),
                Level::WARN => ("\x1b[1;33m", "\x1b[33m"),
                Level::INFO => ("\x1b[1;32m", "\x1b[0m"),
                Level::DEBUG => ("\x1b[1;34m", "\x1b[90m"),
                Level::TRACE => ("\x1b[1;35m", "\x1b[90m"),
            };
            write!(
                writer,
                "\x1b[90m[\x1b[0m\x1b[37m{time}\x1b[0m {lvl_color}{}\x1b[0m\x1b[90m]\x1b[0m",
                level.as_str()
            )?;
            if !msg.is_empty() {
                write!(writer, " {msg_color}{msg}\x1b[0m")?;
            }
            for (k, v) in &fields.pairs {
                write!(writer, " \x1b[36m{k}\x1b[90m=\x1b[37m{v}\x1b[0m")?;
            }
            writeln!(writer)
        } else {
            write!(writer, "[{time} {}]", level.as_str())?;
            if !msg.is_empty() {
                write!(writer, " {msg}")?;
            }
            for (k, v) in &fields.pairs {
                write!(writer, " {k}={v}")?;
            }
            writeln!(writer)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_codes_and_regional_locales() {
        assert_eq!(LogLang::parse("ko"), Some(LogLang::Ko));
        assert_eq!(LogLang::parse("ko_KR.UTF-8"), Some(LogLang::Ko));
        assert_eq!(LogLang::parse("ja-JP"), Some(LogLang::Ja));
        assert_eq!(LogLang::parse("zh_CN"), Some(LogLang::Zh));
        assert_eq!(LogLang::parse("es_ES"), Some(LogLang::Es));
        assert_eq!(LogLang::parse("en_US.UTF-8"), Some(LogLang::En));
        assert_eq!(LogLang::parse("unknown"), None);
    }

    #[test]
    fn translates_core_messages_into_all_supported_languages() {
        let src = "Ferrite listener started";
        assert_eq!(translate_message(LogLang::En, src), src);
        assert_ne!(translate_message(LogLang::Ko, src), src);
        assert_ne!(translate_message(LogLang::Ja, src), src);
        assert_ne!(translate_message(LogLang::Zh, src), src);
        assert_ne!(translate_message(LogLang::Es, src), src);
    }
}
