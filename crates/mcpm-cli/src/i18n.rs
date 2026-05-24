pub use rust_i18n::t;

pub fn init_locale() {
    if let Ok(lang) = std::env::var("MCPM_LANG") {
        rust_i18n::set_locale(&lang);
        return;
    }
    let sys = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    let locale = match sys.replace('_', "-").as_str() {
        s if s.starts_with("zh") => "zh-CN",
        _ => "en",
    };
    rust_i18n::set_locale(locale);
}
