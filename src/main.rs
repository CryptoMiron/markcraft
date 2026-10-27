mod doc;
mod i18n;
mod theme;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, ButtonsType, DialogFlags, FileChooserAction, FileChooserNative,
    MessageDialog, MessageType, ResponseType,
};
use javascriptcore::ValueExt;
use webkit2gtk::{LoadEvent, UserContentManager, UserContentManagerExt, WebView, WebViewExt};

use doc::{Disk, Doc, draft, geometry};
use i18n::{t, tf};
use theme::Theme;

const UI_HTML: &str = include_str!("../ui/built.html");

/// Страница со словарём выбранного языка. Подстановка идёт до бандла:
/// EditorView создаётся уже с переведёнными placeholder и phrases.
fn page_html() -> String {
    UI_HTML.replace("/*__I18N__*/", &i18n::js())
}
const AUTOSAVE: Duration = Duration::from_millis(1200);
const DRAFT_EVERY: Duration = Duration::from_millis(300);
/// Как часто проверяем, не изменил ли файл кто-то снаружи. Дешевле inotify
/// и не требует прав на каталог.
const WATCH_EVERY: Duration = Duration::from_millis(700);

/// Окна, открытые по файлу: путь → окно. Нужен, чтобы повторный запуск
/// с тем же файлом поднимал уже открытое окно, а не плодил копии.
type OpenFiles = Rc<RefCell<Vec<(PathBuf, ApplicationWindow)>>>;

struct App {
    window: ApplicationWindow,
    webview: WebView,
    doc: RefCell<Doc>,
    dirty: Cell<bool>,
    /// Автосейв уже запланирован: второй таймер не нужен, первый возьмёт свежий текст.
    pending: Cell<bool>,
    /// Восстановленный черновик не пишется в файл, пока пользователь не сохранит сам.
    recovered: Cell<bool>,
    /// Черновик уже запланирован: пишем через 300 мс после последней буквы,
    /// а не на каждую (burst из семи символов даёт одну запись).
    draft_pending: Cell<bool>,
    /// Открыт вопрос «сохранить?»: пока пользователь не решил, в файл не пишем.
    asking: Cell<bool>,
    /// Файл изменился снаружи и пользователь ещё не решил, что делать.
    conflict: Cell<bool>,
    /// Про удаление файла уже спросили: второй раз за сессию не спрашиваем.
    gone_reported: Cell<bool>,
    /// Что показать, когда страница догрузилась: черновик важнее файла из argv.
    restore: RefCell<Option<draft::Draft>>,
    initial: Option<PathBuf>,
    /// Палитра и масштаб из Omarchy. Перечитывается при смене темы.
    theme: RefCell<Theme>,
    /// Мониторы каталога темы. Держим здесь: локальная переменная умерла бы
    /// вместе с build_window, и смена темы перестала бы замечаться.
    monitors: RefCell<Vec<gtk::gio::FileMonitor>>,
    /// Приложение и реестр окон: нужны, чтобы Ctrl+N и открытие файла
    /// создавали окно здесь, а не через второй процесс.
    app: Application,
    open_files: OpenFiles,
}

impl App {
    fn refresh_title(&self) {
        let star = if self.dirty.get() { "* " } else { "" };
        self.window
            .set_title(&format!("{star}{} — MarkCraft", self.doc.borrow().name()));
    }

    /// Отдаёт странице палитру и размер текста. Вызывается и при смене темы.
    fn apply_theme(&self) {
        let theme = self.theme.borrow();
        theme::apply_gtk(&theme);
        if std::env::var_os("MC_DEBUG").is_some() {
            eprintln!("css: {}", theme.css());
        }
        let css = format!(
            "{}\n.cm-editor .cm-scroller{{font-size:{}px}}\n.cm-editor .cm-content{{max-width:min({}px, 100%)}}",
            theme.css(),
            theme.scaled(16.0),
            theme.column()
        );
        self.webview.evaluate_javascript(
            &format!("window.mc && window.mc.theme({})", serde_json::json!(css)),
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            |_| {},
        );
    }

    /// Перечитывает тему с диска: Omarchy переключил палитру.
    fn reload_theme(&self) {
        let fresh = Theme::load();
        if *self.theme.borrow() == fresh {
            return;
        }
        *self.theme.borrow_mut() = fresh;
        self.apply_theme();
    }

    fn push(&self) {
        let payload = serde_json::json!({
            "text": self.doc.borrow().text,
            "name": self.doc.borrow().name(),
            "path": self.status_path(),
        });
        self.webview.evaluate_javascript(
            &format!("window.mc.loaded({payload})"),
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            |_| {},
        );
        self.refresh_title();
    }

    /// Путь для статусбара: абсолютный, домашний каталог сокращён до `~`.
    fn status_path(&self) -> Option<String> {
        let doc = self.doc.borrow();
        let path = doc.path()?;
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
            && let Ok(rest) = absolute.strip_prefix(&home)
        {
            return Some(format!("~/{}", rest.display()));
        }
        Some(absolute.display().to_string())
    }

    /// Обновляет только путь в статусбаре: текст и курсор не трогаем.
    fn refresh_path(&self) {
        let path = self.status_path();
        self.webview.evaluate_javascript(
            &format!("window.mc && window.mc.path({})", serde_json::json!(path)),
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            |_| {},
        );
    }

    /// Реестр «путь → окно» должен совпадать с тем, что реально открыто.
    fn remember_path(&self, path: &Path) {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut open = self.open_files.borrow_mut();
        open.retain(|(_, window)| window != &self.window);
        open.retain(|(other, _)| other != &key);
        open.push((key, self.window.clone()));
    }

    /// Новый путь документа: реестр окон и статусбар.
    fn adopt_path(&self, path: PathBuf) {
        self.doc.borrow_mut().set_path(path.clone());
        self.remember_path(&path);
        self.refresh_path();
    }

    /// Ctrl+клик: относительный файл — в MarkCraft, остальное — системе.
    fn open_link(self: &Rc<Self>, href: &str) {
        let base = self
            .doc
            .borrow()
            .path()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok());
        match link_target(base.as_deref(), href) {
            Link::Open(path) if is_doc(&path) => {
                let key = path.canonicalize().unwrap_or(path);
                let current = self.doc.borrow().path().and_then(|p| p.canonicalize().ok());
                if current.as_ref() == Some(&key) {
                    return;
                }
                let open = self.open_files.borrow();
                let existing = open
                    .iter()
                    .find(|(p, _)| p == &key)
                    .map(|(_, window)| window.clone());
                drop(open);
                if let Some(window) = existing {
                    window.present();
                    return;
                }
                build_window(&self.app, Some(key), &self.open_files);
            }
            Link::Open(path) => {
                let uri = gtk::gio::File::for_path(&path).uri();
                let _ = gtk::show_uri_on_window(Some(&self.window), uri.as_str(), 0);
            }
            Link::Missing(path) => {
                let text = tf("error.not_found", &[("path", &path.display().to_string())]);
                error(&self.window, &text);
            }
            Link::External => {
                let _ = gtk::show_uri_on_window(Some(&self.window), href, 0);
            }
        }
    }

    /// Первый показ документа после загрузки страницы.
    fn start(&self) {
        self.apply_theme();
        if let Some(draft) = self.restore.borrow_mut().take() {
            if let Some(path) = &draft.path {
                self.adopt_path(path.clone());
            }
            self.doc.borrow_mut().set_text(&draft.text);
            self.recovered.set(true);
            self.dirty.set(true);
            self.push();
            return;
        }
        if let Some(path) = self.initial.clone() {
            self.load(&path);
            return;
        }
        self.push();
    }

    fn set_text(self: &Rc<Self>, text: &str) {
        self.dirty.set(self.doc.borrow_mut().set_text(text));
        self.refresh_title();
        if !self.dirty.get() {
            return;
        }
        self.schedule_draft();
        self.schedule_save();
    }

    fn schedule_draft(self: &Rc<Self>) {
        if self.draft_pending.get() {
            return;
        }
        self.draft_pending.set(true);
        let later = self.clone();
        gtk::glib::timeout_add_local_once(DRAFT_EVERY, move || {
            later.draft_pending.set(false);
            let _ = draft::write(&later.doc.borrow().text, later.doc.borrow().path());
        });
    }

    fn schedule_save(self: &Rc<Self>) {
        if self.pending.get() {
            return;
        }
        self.pending.set(true);
        let later = self.clone();
        gtk::glib::timeout_add_local_once(AUTOSAVE, move || {
            later.pending.set(false);
            let _ = draft::write(&later.doc.borrow().text, later.doc.borrow().path());
            // shortcut: автосейв в файл только для документа с путём и не восстановленного;
            // watcher внешних изменений — S1b.
            // Условие в отдельную переменную: RefCell-borrow в `if` живёт до конца ветки
            // и паникует на borrow_mut внутри write.
            let has_path = later.doc.borrow().path().is_some();
            if has_path
                && !later.recovered.get()
                && !later.asking.get()
                && !later.conflict.get()
                && !later.gone_reported.get()
            {
                later.write(false);
            }
        });
    }

    fn save(&self) -> bool {
        self.write(true)
    }

    /// show_error = false для автосейва: диалог каждые 1.2 с — не помощь.
    fn write(&self, show_error: bool) -> bool {
        let has_path = self.doc.borrow().path().is_some();
        if !has_path {
            let Some(path) = choose(&self.window, FileChooserAction::Save, t("file.save_as"))
            else {
                return false;
            };
            self.adopt_path(with_md(path));
        }
        // Результат в переменную: borrow_mut в `if let` держится всю ветку, а внутри
        // неё открывается модальный диалог с вложенным циклом событий — таймер
        // автосейва в этот момент паникует на borrow.
        let result = self.doc.borrow_mut().save();
        if let Err(e) = result {
            if show_error {
                error(
                    &self.window,
                    &tf("error.save", &[("error", &e.to_string())]),
                );
            }
            return false;
        }
        draft::clear();
        self.recovered.set(false);
        self.gone_reported.set(false);
        self.dirty.set(false);
        self.refresh_title();
        true
    }

    fn save_as(&self) {
        let Some(path) = choose(&self.window, FileChooserAction::Save, t("file.save_as")) else {
            return;
        };
        self.adopt_path(with_md(path));
        self.save();
    }

    fn load(&self, path: &Path) {
        match Doc::open(path) {
            Ok(doc) => {
                let same_dir = self
                    .doc
                    .borrow()
                    .path()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    == path.parent().map(|d| d.to_path_buf());
                *self.doc.borrow_mut() = doc;
                self.remember_path(path);
                self.gone_reported.set(false);
                // Всё, что было в редакторе, заменено диском: несохранённого нет,
                // и черновик от прежнего буфера больше не нужен.
                self.dirty.set(false);
                self.recovered.set(false);
                draft::clear();
                if same_dir {
                    self.push();
                } else {
                    // Картинки резолвятся от папки документа: другая папка — другая база.
                    self.reload_page();
                }
            }
            Err(e) => error(&self.window, &format!("{}: {e}", path.display())),
        }
    }

    /// Перезагружает страницу с базой текущего документа и снова отдаёт текст.
    fn reload_page(&self) {
        let base = self
            .doc
            .borrow()
            .path()
            .and_then(|p| p.parent().map(|d| format!("file://{}/", d.display())));
        self.webview.load_html(&page_html(), base.as_deref());
    }

    /// Ctrl+= / Ctrl+- / Ctrl+0: 10% за шаг, 0 — сброс к масштабу рабочего стола.
    fn zoom(&self, step: i32) {
        let mut theme = self.theme.borrow_mut();
        let next = match step {
            0 => 1.0,
            s if s > 0 => theme.zoom + 0.1,
            _ => theme.zoom - 0.1,
        };
        theme.set_zoom(next);
        drop(theme);
        self.apply_theme();
    }

    /// Ctrl+N: новое пустое окно в этом же процессе.
    fn new_window(&self) {
        build_window(&self.app, None, &self.open_files);
    }

    fn open_dialog(&self) {
        if !self.maybe_save() {
            return;
        }
        if let Some(path) = choose(&self.window, FileChooserAction::Open, t("file.open")) {
            self.load(&path);
        }
    }

    /// Раз в WATCH_EVERY смотрим на файл. Модальный диалог крутит вложенный цикл
    /// событий, поэтому таймер продолжает тикать — флаги conflict/asking гасят его.
    fn watch_disk(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        gtk::glib::timeout_add_local(WATCH_EVERY, move || {
            if let Some(app) = weak.upgrade() {
                app.check_disk();
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    fn check_disk(self: &Rc<Self>) {
        if self.asking.get() || self.conflict.get() || self.recovered.get() {
            return;
        }
        if self.doc.borrow().path().is_none() {
            return;
        }
        let state = self.doc.borrow().disk_state();
        match state {
            Disk::Same => self.gone_reported.set(false),
            Disk::Modified => {
                // Без своих правок терять нечего: молча берём версию с диска.
                // Спрашивать вредно — «оставить моё» оставило бы в буфере
                // устаревший текст, который следующий автосейв записал бы поверх чужого.
                if self.dirty.get() {
                    self.resolve_external(false);
                } else {
                    let path = self.doc.borrow().path().map(Path::to_path_buf);
                    if let Some(path) = path {
                        self.load(&path);
                    }
                }
            }
            Disk::Gone => {
                if !self.gone_reported.get() {
                    self.resolve_external(true);
                }
            }
        }
    }

    /// true — файл исчез. Перечитать нельзя, поэтому «сохранить заново».
    fn resolve_external(self: &Rc<Self>, gone: bool) {
        self.conflict.set(true);
        let reload = if gone {
            ask_dialog(
                &self.window,
                t("external.gone"),
                t("common.save_again"),
                t("common.keep_as_is"),
            )
        } else {
            ask_dialog(
                &self.window,
                t("external.changed"),
                t("common.reread"),
                t("common.keep_mine"),
            )
        };
        self.conflict.set(false);

        if reload {
            if gone {
                // Перечитывать нечего: возвращаем файл из того, что в редакторе.
                self.gone_reported.set(false);
                self.write(true);
            } else {
                let path = self.doc.borrow().path().map(Path::to_path_buf);
                if let Some(path) = path {
                    self.load(&path);
                }
            }
        } else {
            // «Оставить моё»: чужая запись принята как состояние диска,
            // иначе вопрос повторялся бы каждые 700 мс. Автосейв, который
            // отработал впустую за время диалога, взводим заново — иначе
            // «оставить моё» осталось бы только в памяти.
            self.doc.borrow_mut().accept_disk();
            // Удалённый файл не пересоздаём молча: автосейв ждёт явного Ctrl+S.
            self.gone_reported.set(gone);
            if self.dirty.get() && !gone {
                self.schedule_save();
            }
        }
    }

    /// Спрашивает про несохранённое. false — пользователь передумал.
    fn maybe_save(&self) -> bool {
        if !self.dirty.get() {
            draft::clear();
            return true;
        }
        self.asking.set(true);
        let response = ask_save_changes(&self.window);
        self.asking.set(false);
        match response {
            ResponseType::Yes => self.save(),
            // «Не сохранять» значит и «не восстанавливать»: иначе выброшенный
            // текст всплывёт при следующем запуске.
            ResponseType::No => {
                draft::clear();
                true
            }
            _ => false,
        }
    }
}

fn with_md(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("md")
    } else {
        path
    }
}

enum Link {
    Open(PathBuf),
    Missing(PathBuf),
    External,
}

/// Куда ведёт Ctrl+клик: локальный файл, отсутствующий файл или внешний адрес.
fn link_target(base: Option<&Path>, href: &str) -> Link {
    let raw = href.split('#').next().unwrap_or_default();
    if raw.is_empty()
        || raw.contains("://")
        || raw.starts_with("mailto:")
        || raw.starts_with("tel:")
    {
        return Link::External;
    }
    let path = Path::new(raw);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base) = base {
        base.join(path)
    } else {
        path.to_path_buf()
    };
    if path.exists() {
        Link::Open(path)
    } else {
        Link::Missing(path)
    }
}

fn is_doc(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "txt"))
}

fn choose(parent: &ApplicationWindow, action: FileChooserAction, title: &str) -> Option<PathBuf> {
    let native = FileChooserNative::new(Some(title), Some(parent), action, None, None);
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Markdown"));
    for pattern in ["*.md", "*.markdown", "*.txt"] {
        filter.add_pattern(pattern);
    }
    native.add_filter(filter);
    if native.run() == ResponseType::Accept {
        native.file().and_then(|f| f.path())
    } else {
        None
    }
}

fn ask_save_changes(parent: &ApplicationWindow) -> ResponseType {
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Question,
        ButtonsType::None,
        t("unsaved.message"),
    );
    dialog.add_button(t("common.cancel"), ResponseType::Cancel);
    dialog.add_button(t("common.dont_save"), ResponseType::No);
    dialog.add_button(t("common.save"), ResponseType::Yes);
    dialog.set_default_response(ResponseType::Yes);
    let response = dialog.run();
    dialog.close();
    response
}

/// Две кнопки, первая — действие по умолчанию. true — нажата первая.
fn ask_dialog(parent: &ApplicationWindow, text: &str, accept: &str, reject: &str) -> bool {
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Question,
        ButtonsType::None,
        text,
    );
    dialog.add_button(reject, ResponseType::No);
    dialog.add_button(accept, ResponseType::Yes);
    dialog.set_default_response(ResponseType::Yes);
    let response = dialog.run();
    dialog.close();
    response == ResponseType::Yes
}

/// Ctrl+? / F1: список клавиш. Один диалог вместо отдельного окна.
fn help(parent: &ApplicationWindow) {
    // Текст простой, без разметки: set_secondary_markup в этом наборе
    // фич gtk-rs недоступен, а ради заголовков заводить свой Dialog — лишний код.
    let keys = t("help.body");
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Info,
        ButtonsType::None,
        keys,
    );
    dialog.set_title(t("help.title"));
    dialog.add_button(t("common.close"), ResponseType::Close);
    dialog.set_default_response(ResponseType::Close);
    dialog.run();
    dialog.close();
}

/// Копирует документ как HTML с текстовым запасным вариантом: вставка
/// в почту или редактор сохранит оформление, в терминал — текст.
fn copy_html(html: &str, text: &str) {
    use gtk::{TargetEntry, TargetFlags};
    let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
    let targets = [
        TargetEntry::new("text/html", TargetFlags::empty(), 0),
        TargetEntry::new("text/plain;charset=utf-8", TargetFlags::empty(), 1),
        TargetEntry::new("UTF8_STRING", TargetFlags::empty(), 1),
        TargetEntry::new("STRING", TargetFlags::empty(), 1),
        TargetEntry::new("TEXT", TargetFlags::empty(), 1),
    ];
    let html = html.to_owned();
    let text = text.to_owned();
    clipboard.set_with_data(&targets, move |_, selection, info| {
        if info == 0 {
            selection.set(&selection.target(), 8, html.as_bytes());
        } else {
            selection.set_text(&text);
        }
    });
}

fn error(parent: &ApplicationWindow, text: &str) {
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Error,
        ButtonsType::None,
        text,
    );
    dialog.add_button(t("common.close"), ResponseType::Close);
    dialog.set_default_response(ResponseType::Close);
    dialog.run();
    dialog.close();
}

fn build_window(app: &Application, initial: Option<PathBuf>, open_files: &OpenFiles) {
    let (width, height) = geometry::read().unwrap_or((980, 760));
    let window = ApplicationWindow::builder()
        .application(app)
        .title("MarkCraft")
        .default_width(width)
        .default_height(height)
        .build();
    // В тайлинге Hyprland окно обязано ужиматься: 720 px минимума мешали
    // сузить окно и провоцировали «сплющивание» содержимого.
    window.set_size_request(280, 200);

    let manager = UserContentManager::new();
    manager.register_script_message_handler("ipc");

    let webview = WebView::builder().user_content_manager(&manager).build();

    let state = Rc::new(App {
        window: window.clone(),
        webview: webview.clone(),
        doc: RefCell::new(Doc::empty()),
        dirty: Cell::new(false),
        pending: Cell::new(false),
        recovered: Cell::new(false),
        asking: Cell::new(false),
        conflict: Cell::new(false),
        gone_reported: Cell::new(false),
        draft_pending: Cell::new(false),
        restore: RefCell::new(draft::take()),
        initial,
        theme: RefCell::new(Theme::load()),
        monitors: RefCell::new(Vec::new()),
        app: app.clone(),
        open_files: open_files.clone(),
    });

    theme::apply_gtk(&state.theme.borrow());

    let sender = state.clone();
    manager.connect_script_message_received(Some("ipc"), move |_, result| {
        let Some(value) = result.js_value() else {
            return;
        };
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&value.to_str()) else {
            return;
        };
        let text = message["text"].as_str().unwrap_or_default();
        match message["type"].as_str().unwrap_or_default() {
            "dirty" => sender.set_text(text),
            "save" => {
                sender.set_text(text);
                sender.save();
            }
            "saveAs" => {
                sender.set_text(text);
                sender.save_as();
            }
            "open" => sender.open_dialog(),
            "paste" => {
                // Вставку берём из GTK-буфера: WebKit под Wayland вставку
                // по синтетическому Ctrl+V не делает, а буфер читается всегда.
                let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
                if let Some(text) = clipboard.wait_for_text() {
                    let payload = serde_json::json!({ "text": text.as_str() });
                    sender.webview.evaluate_javascript(
                        &format!("window.mc.insert({payload})"),
                        None,
                        None,
                        None::<&gtk::gio::Cancellable>,
                        |_| {},
                    );
                }
            }
            "zoom" => {
                let step = message["step"].as_i64().unwrap_or(0) as i32;
                sender.zoom(step);
            }
            "newWindow" => sender.new_window(),
            "help" => help(&sender.window),
            "copyHtml" => {
                let html = message["html"].as_str().unwrap_or_default();
                let text = message["text"].as_str().unwrap_or_default();
                copy_html(html, text);
            }
            "copyText" => {
                let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
                clipboard.set_text(text);
                // store(): иначе после закрытия окна буфер опустеет.
                clipboard.store();
            }
            "openUrl" => {
                let url = message["url"].as_str().unwrap_or_default();
                if !url.is_empty() {
                    sender.open_link(url);
                }
            }
            _ => {}
        }
    });

    let painter = state.clone();
    webview.connect_load_changed(move |_, event| {
        if event == LoadEvent::Finished {
            painter.start();
        }
    });

    let closer = state.clone();
    window.connect_delete_event(move |window, _| {
        if !closer.maybe_save() {
            return gtk::glib::Propagation::Stop;
        }
        let (width, height) = window.size();
        geometry::write(width, height);
        gtk::glib::Propagation::Proceed
    });

    state.watch_disk();

    // Смена темы в Omarchy: перекрашиваем без рестарта. Weak, а не Rc:
    // иначе монитор держит состояние, которое держит монитор.
    let repaint = Rc::downgrade(&state);
    *state.monitors.borrow_mut() = theme::watch(move || {
        if let Some(app) = repaint.upgrade() {
            app.reload_theme();
        }
    });

    // Относительные картинки должны находиться рядом с документом.
    // Для безымянного черновика базы нет — картинки покажутся плейсхолдером.
    let base = state
        .doc
        .borrow()
        .path()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .or_else(|| {
            state
                .initial
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        })
        .map(|d| format!("file://{}/", d.display()));

    // `initial` уже переехал в состояние; для реестра берём путь оттуда.
    if let Some(path) = state.initial.clone() {
        let key = path.canonicalize().unwrap_or(path);
        open_files.borrow_mut().push((key, window.clone()));
    }

    // Закрытое окно не должно оставаться в реестре: иначе повторное открытие
    // того же файла попытается поднять мёртвое окно.
    let registry = open_files.clone();
    window.connect_destroy(move |window| {
        registry.borrow_mut().retain(|(_, w)| w != window);
    });

    window.add(&webview);
    webview.load_html(&page_html(), base.as_deref());
    window.show_all();
    // Новое окно должно не только появиться, но и получить фокус: иначе
    // первый клик по нему уходит на активацию окна, а не в редактор, и
    // выделение мышью сбрасывается.
    window.present();
    webview.grab_focus();
    let focus = webview.clone();
    window.connect_focus_in_event(move |_, _| {
        focus.grab_focus();
        gtk::glib::Propagation::Proceed
    });
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("markcraft [FILE]");
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("markcraft {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .collect();

    // HANDLES_OPEN: GApplication сам передаёт файлы уже запущенному экземпляру
    // через сигнал `open`. Без флага второй запуск слал только `activate`,
    // и приложение открывало прежний файл вместо нового.
    let app = Application::builder()
        .application_id("app.markcraft.MarkCraft")
        .flags(gtk::gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    let open_files: OpenFiles = Rc::new(RefCell::new(Vec::new()));

    let registry = open_files.clone();
    app.connect_activate(move |app| build_window(app, None, &registry));

    let registry = open_files.clone();
    app.connect_open(move |app, files, _| {
        for file in files {
            let Some(path) = file.path() else { continue };
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());

            // Тот же файл уже открыт — поднимаем его окно, а не второе.
            let existing = registry
                .borrow()
                .iter()
                .find(|(p, _)| *p == key)
                .map(|(_, w)| w.clone());
            if let Some(window) = existing {
                window.present();
                continue;
            }

            build_window(app, Some(path), &registry);
        }
    });

    // Первый элемент — имя программы, как в argv: остальное GApplication
    // разбирает сам и решает, слать `open` или `activate`.
    let mut argv: Vec<String> = vec!["markcraft".to_owned()];
    argv.extend(files.iter().map(|p| p.to_string_lossy().into_owned()));
    app.run_with_args(&argv);
}

#[cfg(test)]
mod link_tests {
    use super::*;
    use std::fs;

    #[test]
    fn relative_links_resolve_against_the_document() {
        let dir = std::env::temp_dir().join(format!("markcraft-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/SPEC.md"), "# spec\n").unwrap();

        match link_target(Some(&dir), "docs/SPEC.md") {
            Link::Open(path) => assert_eq!(path, dir.join("docs/SPEC.md")),
            _ => panic!("относительная ссылка не нашла файл"),
        }
        assert!(matches!(
            link_target(Some(&dir), "docs/SPEC.md#r4"),
            Link::Open(_)
        ));
        assert!(matches!(link_target(Some(&dir), "docs"), Link::Open(_)));
        assert!(matches!(
            link_target(Some(&dir), "docs/missing.md"),
            Link::Missing(_)
        ));
        assert!(matches!(
            link_target(Some(&dir), "https://example.com"),
            Link::External
        ));
        assert!(matches!(link_target(Some(&dir), "#r4"), Link::External));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_text_documents_open_in_markcraft() {
        assert!(is_doc(Path::new("README.md")));
        assert!(is_doc(Path::new("SKILL.MD")));
        assert!(is_doc(Path::new("notes.txt")));
        assert!(!is_doc(Path::new("pic.png")));
        assert!(!is_doc(Path::new("src/main.rs")));
    }
}
