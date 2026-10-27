use std::fs;
use std::path::PathBuf;

use gtk::gio;
use gtk::prelude::*;

/// Палитра и размер текста из Omarchy. Своей темы у приложения нет (SPEC I5).
#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    pub dark: bool,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    /// Цвет выделения из `colors.toml` — фон выделенного текста и поиска.
    pub selection: String,
    pub muted: String,
    pub code_background: String,
    /// Что просит рабочий стол (GNOME `text-scaling-factor`), 12 px = 1.0.
    pub text_scale: f64,
    /// Что попросил пользователь через Ctrl+= / Ctrl+-, поверх рабочего стола.
    pub zoom: f64,
}

impl Default for Theme {
    fn default() -> Self {
        // Тёмная палитра по умолчанию: Omarchy по умолчанию тёмный, а если
        // файла темы нет, лучше не мигать белым.
        Self {
            dark: true,
            background: "#1a1b1b".into(),
            foreground: "#eeeeee".into(),
            accent: "#7aa2f7".into(),
            selection: "#2f3549".into(),
            muted: "#909191".into(),
            code_background: "#1c1c1c".into(),
            text_scale: 1.0,
            zoom: 1.0,
        }
    }
}

fn theme_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("omarchy/current/theme"))
}

/// Минимальный разбор `colors.toml`: `key = "value"`, комментарии и секции
/// не интересуют. Полноценный TOML здесь был бы лишней зависимостью.
fn parse_colors(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'').trim();
        if !value.is_empty() {
            map.insert(key.trim().to_owned(), value.to_owned());
        }
    }
    map
}

/// `#rrggbb` → яркость. Невалидный цвет даёт None, чтобы не портить контраст.
fn luminance(color: &str) -> Option<f64> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| {
        u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .ok()
            .map(|v| v as f64 / 255.0)
    };
    Some(0.299 * channel(0)? + 0.587 * channel(1)? + 0.114 * channel(2)?)
}

/// Сумма разниц каналов: грубая, но достаточная мера «видно или нет».
fn contrast(a: &str, b: &str) -> i32 {
    let parse = |c: &str| -> Option<(i32, i32, i32)> {
        let hex = c.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let ch = |i: usize| i32::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok();
        Some((ch(0)?, ch(1)?, ch(2)?))
    };
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => (a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs(),
        _ => 0,
    }
}

/// Смешивает цвет с текстом: панель кода не должна сливаться с фоном.
fn blend(base: &str, other: &str, amount: f64) -> String {
    let parse = |c: &str| -> Option<(f64, f64, f64)> {
        let hex = c.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let ch = |i: usize| {
            u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .ok()
                .map(|v| v as f64)
        };
        Some((ch(0)?, ch(1)?, ch(2)?))
    };
    let (Some(a), Some(b)) = (parse(base), parse(other)) else {
        return base.to_owned();
    };
    let mix = |x: f64, y: f64| (x + (y - x) * amount).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(a.0, b.0),
        mix(a.1, b.1),
        mix(a.2, b.2)
    )
}

impl Theme {
    /// Читает `colors.toml` и масштаб текста. Любая ошибка — дефолт, а не паника.
    pub fn load() -> Self {
        let mut theme = Theme::default();
        if let Some(dir) = theme_dir()
            && let Ok(text) = fs::read_to_string(dir.join("colors.toml"))
        {
            theme.apply_colors(&text);
        }
        theme.text_scale = read_text_scale();
        theme.zoom = read_zoom();
        theme
    }

    /// Масштаб пользователя, запоминается между запусками.
    pub fn set_zoom(&mut self, zoom: f64) {
        self.zoom = zoom.clamp(0.5, 3.0);
        let _ = write_zoom(self.zoom);
    }

    fn apply_colors(&mut self, text: &str) {
        let map = parse_colors(text);

        let background = map.get("background").filter(|c| luminance(c).is_some());
        let foreground = map.get("foreground").filter(|c| luminance(c).is_some());

        // Режим берём из файла, а если его нет — из яркости фона.
        self.dark = match map.get("mode").map(String::as_str) {
            Some("dark") => true,
            Some("light") => false,
            _ => background
                .and_then(|c| luminance(c))
                .map(|l| l < 0.5)
                .unwrap_or(true),
        };

        if let Some(c) = background {
            self.background = c.clone();
        }
        if let Some(c) = foreground {
            self.foreground = c.clone();
        }
        if let Some(c) = map.get("accent").filter(|c| luminance(c).is_some()) {
            self.accent = c.clone();
        }
        if let Some(c) = map.get("muted").filter(|c| luminance(c).is_some()) {
            self.muted = c.clone();
        }
        if let Some(c) = map
            .get("selection_background")
            .filter(|c| luminance(c).is_some())
        {
            self.selection = c.clone();
        }
        if let Some(c) = map.get("selection").filter(|c| luminance(c).is_some()) {
            self.selection = c.clone();
        }

        // Панель кода: lighter_background темы, иначе лёгкая примесь текста.
        // Примесь, а не осветление: на чисто чёрном фоне осветление не видно.
        self.code_background = map
            .get("lighter_background")
            .filter(|c| luminance(c).is_some() && *c != &self.background)
            .cloned()
            .unwrap_or_else(|| blend(&self.background, &self.foreground, 0.06));
    }

    /// Токены подсветки зависят от фона блока кода, а не от темы окна:
    /// в светлой теме Omarchy панель кода тоже светлая, и светлые токены
    /// на ней не читаются.
    fn token_css(&self) -> String {
        let dark_panel = luminance(&self.code_background)
            .map(|l| l < 0.5)
            .unwrap_or(true);
        let (fg, comment, string, number, keyword, ty, func, prop, op, meta) = if dark_panel {
            (
                "#e8e8e3", "#7d8590", "#86efac", "#fdba74", "#d8b4fe", "#93c5fd", "#7dd3fc",
                "#c4b5fd", "#a1a1aa", "#fcd34d",
            )
        } else {
            (
                "#2d2d28", "#8a8f98", "#0a7d5a", "#b45309", "#a21caf", "#1d4ed8", "#0e7490",
                "#7c3aed", "#52525b", "#b45309",
            )
        };
        format!(
            "--mc-code-fg:{fg};\
--mc-tok-comment:{comment};--mc-tok-string:{string};--mc-tok-number:{number};\
--mc-tok-keyword:{keyword};--mc-tok-type:{ty};--mc-tok-func:{func};\
--mc-tok-prop:{prop};--mc-tok-op:{op};--mc-tok-punct:{op};--mc-tok-meta:{meta};"
        )
    }

    /// Цвет, сдвинутый к тексту, пока он не станет заметным на обоих фонах:
    /// тема задаёт цвета под своё окно, а не под панель кода.
    fn visible(&self, base: &str) -> String {
        let mut amount = 0.0;
        let mut color = base.to_owned();
        // Шаг 0.1, предел 0.7. Порог 140 из 765: при 45 разница формально есть,
        // но глазом не видна — на этом и потеряли выделение в первый раз.
        while amount < 0.7
            && (contrast(&color, &self.background) < 140
                || contrast(&color, &self.code_background) < 140)
        {
            amount += 0.1;
            color = blend(base, &self.foreground, amount);
        }
        color
    }

    /// Подсветка совпадений поиска: `selection` из colors.toml, но заметная.
    fn search_css(&self) -> String {
        self.visible(&self.selection)
    }

    /// CSS-переменные для страницы. Одна строка — один слой правды о цвете.
    pub fn css(&self) -> String {
        let border = blend(&self.background, &self.foreground, 0.2);
        let inline_bg = blend(&self.background, &self.foreground, 0.07);
        let table_head = blend(&self.background, &self.foreground, 0.12);
        // Всё внутри одного `:root{}`: объявление переменной на верхнем уровне
        // CSS-парсер молча выбрасывает, и подсветка кода остаётся без цветов.
        format!(
            ":root{{color-scheme:{};--mc-bg:{};--mc-fg:{};--mc-accent:{};\
--mc-muted:{};--mc-border:{};--mc-inline-bg:{};--mc-code-bg:{};\
--mc-selection:{};--mc-search:{};--mc-table-head:{};{}}}",
            if self.dark { "dark" } else { "light" },
            self.background,
            self.foreground,
            self.accent,
            self.muted,
            border,
            inline_bg,
            self.code_background,
            self.selection,
            self.search_css(),
            table_head,
            self.token_css(),
        )
    }

    /// Размер текста: база × рабочий стол × масштаб пользователя.
    pub fn scaled(&self, px: f64) -> f64 {
        (px * self.text_scale * self.zoom).round()
    }

    /// Колонка должна расти вместе с текстом: 700 px при 16 px — это
    /// ~44 знака, и при 20 px те же 700 px дают уже 35.
    pub fn column(&self) -> f64 {
        self.scaled(700.0)
    }
}

fn zoom_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("markcraft").join("zoom"))
}

fn read_zoom() -> f64 {
    zoom_file()
        .and_then(|f| fs::read_to_string(f).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|z| z.clamp(0.5, 3.0))
        .unwrap_or(1.0)
}

fn write_zoom(zoom: f64) -> std::io::Result<()> {
    let Some(file) = zoom_file() else {
        return Ok(());
    };
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, format!("{zoom}"))
}

/// `org.gnome.desktop.interface text-scaling-factor` через портал настроек.
/// Портал может отсутствовать или молчать — тогда 1.0.
fn read_text_scale() -> f64 {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return 1.0;
    };
    let reply = connection.call_sync(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
        "Read",
        Some(&("org.gnome.desktop.interface", "text-scaling-factor").to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        150,
        None::<&gio::Cancellable>,
    );
    let Ok(reply) = reply else { return 1.0 };
    // Портал отвечает `(<<1.25>>,)`: два уровня вариантов вокруг double.
    // Разворачиваем, пока разворачивается, — сколько именно уровней, зависит
    // от версии портала.
    // Ответ — кортеж из одного элемента: `(v)`.
    let mut variant = reply.child_value(0);
    // `as_variant` на не-варианте пишет GLib-CRITICAL, поэтому проверяем тип.
    while variant.type_().as_str() == "v" {
        let Some(inner) = variant.as_variant() else {
            break;
        };
        variant = inner;
    }
    variant
        .get::<f64>()
        .map(|s| s.clamp(0.5, 3.0))
        .unwrap_or(1.0)
}

/// Следит за каталогом темы: Omarchy меняет симлинк `current`, поэтому
/// смотреть надо на `current`, а не на сам `colors.toml`.
pub fn watch<F: Fn() + Clone + 'static>(on_change: F) -> Vec<gio::FileMonitor> {
    let Some(base) = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
    else {
        return Vec::new();
    };

    // Omarchy меняет и симлинк `current/theme`, и содержимое самого каталога
    // темы, поэтому слушаем оба уровня: какой сработает, тот и перекрасит.
    let mut monitors = Vec::new();
    for dir in [
        base.join("omarchy/current"),
        base.join("omarchy/current/theme"),
    ] {
        if !dir.exists() {
            continue;
        }
        let Ok(monitor) = gio::File::for_path(&dir)
            .monitor_directory(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
        else {
            continue;
        };
        let callback = on_change.clone();
        monitor.connect_changed(move |_, _, _, _| callback());
        monitors.push(monitor);
    }
    monitors
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKYO: &str = r##"
# Generated by Omarchy
mode = "dark"

accent = "#7aa2f7"
selection = "#292e42"
selection_foreground = "#c0caf5"
muted = "#414868"

background = "#1a1b26"
lighter_background = "#24283b"
foreground = "#a9b1d6"
bright_foreground = "#c0caf5"
"##;

    #[test]
    fn parses_omarchy_theme() {
        let mut theme = Theme::default();
        theme.apply_colors(TOKYO);
        assert!(theme.dark);
        assert_eq!(theme.background, "#1a1b26");
        assert_eq!(theme.foreground, "#a9b1d6");
        assert_eq!(theme.accent, "#7aa2f7");
        assert_eq!(theme.code_background, "#24283b");
        assert_eq!(theme.selection, "#292e42");
    }

    #[test]
    fn selection_uses_theme_colour() {
        let mut theme = Theme::default();
        theme.apply_colors(TOKYO);
        let css = theme.css();
        assert!(css.contains("--mc-selection:#292e42"), "{css}");
        // Цвет текста на выделении не задаём: ::selection красит только фон.
        assert!(!css.contains("--mc-selection-fg"), "{css}");
        // Поиск остаётся на подмешанном цвете, а не на сыром выделении темы.
        assert!(css.contains("--mc-search:#"), "{css}");
    }

    #[test]
    fn light_theme_is_light() {
        let mut theme = Theme::default();
        theme
            .apply_colors("mode = \"light\"\nbackground = \"#ffffff\"\nforeground = \"#222324\"\n");
        assert!(!theme.dark);
        assert_eq!(theme.background, "#ffffff");
    }

    #[test]
    fn mode_falls_back_to_background_luminance() {
        let mut theme = Theme::default();
        theme.apply_colors("background = \"#ffffff\"\nforeground = \"#222324\"\n");
        assert!(!theme.dark, "белый фон без mode должен дать светлую тему");

        let mut theme = Theme::default();
        theme.apply_colors("background = \"#01061b\"\nforeground = \"#EDEADD\"\n");
        assert!(theme.dark);
    }

    #[test]
    fn broken_values_keep_defaults() {
        let mut theme = Theme::default();
        let before = theme.clone();
        theme.apply_colors("background = \"не цвет\"\naccent = \"#12\"\nforeground =\n");
        assert_eq!(theme.background, before.background);
        assert_eq!(theme.accent, before.accent);
        assert_eq!(theme.foreground, before.foreground);
    }

    #[test]
    fn missing_lighter_background_is_mixed() {
        let mut theme = Theme::default();
        theme.apply_colors("background = \"#000000\"\nforeground = \"#ffffff\"\n");
        // На чисто чёрном осветление не помогло бы, примесь белого — да.
        assert_ne!(theme.code_background, "#000000");
        assert!(luminance(&theme.code_background).unwrap() > 0.0);
    }

    #[test]
    fn lighter_background_equal_to_page_is_ignored() {
        let mut theme = Theme::default();
        theme.apply_colors("background = \"#1a1b1b\"\nforeground = \"#eeeeee\"\nlighter_background = \"#1a1b1b\"\n");
        assert_ne!(theme.code_background, "#1a1b1b");
    }

    #[test]
    fn text_scale_is_clamped() {
        // Значение из портала может быть каким угодно; вёрстка не должна ломаться.
        assert!((0.5..=3.0).contains(&Theme::default().text_scale.clamp(0.5, 3.0)));
    }

    #[test]
    fn tokens_follow_code_panel_brightness() {
        // Тёмная тема: панель тёмная, токены светлые.
        let mut theme = Theme::default();
        theme.apply_colors("mode = \"dark\"\nbackground = \"#1a1b1b\"\nforeground = \"#eeeeee\"\n");
        assert!(theme.css().contains("--mc-tok-string:#86efac"));
        assert!(theme.css().contains("--mc-code-fg:#e8e8e3"));

        // Светлая тема: панель светлая, токены тёмные.
        let mut theme = Theme::default();
        theme.apply_colors(
            "mode = \"light\"\nbackground = \"#eff1f5\"\nforeground = \"#4c4f69\"\nlighter_background = \"#ccd0da\"\n",
        );
        assert!(theme.css().contains("--mc-tok-string:#0a7d5a"));
        assert!(theme.css().contains("--mc-code-fg:#2d2d28"));
    }

    #[test]
    fn column_grows_with_text_scale() {
        let at = |scale| Theme {
            text_scale: scale,
            ..Default::default()
        };
        assert_eq!(at(1.0).column(), 700.0);
        assert_eq!(at(1.25).column(), 875.0);
        assert_eq!(at(0.5).column(), 350.0);
    }

    #[test]
    fn search_is_visible_on_page_and_on_code() {
        for (name, colors) in [
            (
                "tokyo-night",
                "mode = \"dark\"\nbackground = \"#1a1b26\"\nforeground = \"#a9b1d6\"\n\
selection = \"#292e42\"\nlighter_background = \"#24283b\"\n",
            ),
            (
                "catppuccin-latte",
                "mode = \"light\"\nbackground = \"#eff1f5\"\nforeground = \"#4c4f69\"\n\
selection = \"#ccd0da\"\n",
            ),
        ] {
            let mut theme = Theme::default();
            theme.apply_colors(colors);
            let selection = theme.search_css();
            assert!(
                contrast(&selection, &theme.background) >= 140,
                "{name}: поиск {selection} сливается с фоном {}",
                theme.background
            );
            assert!(
                contrast(&selection, &theme.code_background) >= 140,
                "{name}: поиск {selection} сливается с панелью кода {}",
                theme.code_background
            );
        }
    }

    #[test]
    fn search_stays_close_to_theme_colour() {
        // Если в теме выделение уже контрастное, его не трогаем.
        let mut theme = Theme::default();
        theme.apply_colors(
            "mode = \"dark\"\nbackground = \"#000000\"\nforeground = \"#ffffff\"\n\
selection = \"#4466aa\"\n",
        );
        assert_eq!(theme.search_css(), "#4466aa");
    }

    #[test]
    fn every_variable_is_inside_root() {
        // Переменная вне `:root{}` не работает: парсер её выбрасывает.
        let css = Theme::default().css();
        let root = css
            .strip_prefix(":root{")
            .and_then(|s| s.strip_suffix('}'))
            .expect("CSS должен быть одним правилом :root");
        for var in [
            "--mc-bg",
            "--mc-code-fg",
            "--mc-tok-string",
            "--mc-tok-keyword",
        ] {
            assert!(root.contains(var), "{var} вне :root");
        }
        assert!(
            !css.trim_end_matches('}').contains("}"),
            "внутри :root лишняя скобка"
        );
    }

    #[test]
    fn zoom_multiplies_desktop_scale() {
        let theme = Theme {
            text_scale: 1.25,
            zoom: 2.0,
            ..Default::default()
        };
        assert_eq!(theme.scaled(16.0), 40.0);
        assert_eq!(theme.column(), 1750.0);
    }

    #[test]
    fn css_has_every_variable() {
        let css = Theme::default().css();
        for var in [
            "--mc-bg",
            "--mc-fg",
            "--mc-accent",
            "--mc-muted",
            "--mc-border",
            "--mc-inline-bg",
            "--mc-code-bg",
            "--mc-selection",
            "--mc-search",
            "--mc-table-head",
        ] {
            assert!(css.contains(var), "в CSS нет {var}");
        }
    }
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    #[test]
    fn blend_is_gray_between_page_and_text() {
        // Тёмный фон + светлый текст = серый, без цветного сдвига.
        let mixed = blend("#1a1b1b", "#eeeeee", 0.2);
        let lum = luminance(&mixed).unwrap();
        assert!(lum > 0.2 && lum < 0.4, "ожидали серый, получили {mixed}");
        let hex = mixed.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
        assert!(
            (r as i32 - g as i32).abs() <= 2,
            "r={r} g={g} b={b} в {mixed}"
        );
        assert!(
            (g as i32 - b as i32).abs() <= 2,
            "r={r} g={g} b={b} в {mixed}"
        );
    }
}
