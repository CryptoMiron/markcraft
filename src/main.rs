mod doc;
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
use theme::Theme;

const UI_HTML: &str = include_str!("../ui/built.html");
const AUTOSAVE: Duration = Duration::from_millis(1200);
const DRAFT_EVERY: Duration = Duration::from_millis(300);
/// Как часто проверяем, не изменил ли файл кто-то снаружи. Дешевле inotify
/// и не требует прав на каталог.
const WATCH_EVERY: Duration = Duration::from_millis(700);

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

    /// Первый показ документа после загрузки страницы.
    fn start(&self) {
        self.apply_theme();
        if let Some(draft) = self.restore.borrow_mut().take() {
            if let Some(path) = &draft.path {
                self.doc.borrow_mut().set_path(path.clone());
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
            let Some(path) = choose(&self.window, FileChooserAction::Save, "Сохранить как")
            else {
                return false;
            };
            self.doc.borrow_mut().set_path(with_md(path));
        }
        // Результат в переменную: borrow_mut в `if let` держится всю ветку, а внутри
        // неё открывается модальный диалог с вложенным циклом событий — таймер
        // автосейва в этот момент паникует на borrow.
        let result = self.doc.borrow_mut().save();
        if let Err(e) = result {
            if show_error {
                error(
                    &self.window,
                    &format!(
                        "Не удалось сохранить: {e}. Ctrl+Shift+S — сохранить под другим именем."
                    ),
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
        let Some(path) = choose(&self.window, FileChooserAction::Save, "Сохранить как")
        else {
            return;
        };
        self.doc.borrow_mut().set_path(with_md(path));
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
        self.webview.load_html(UI_HTML, base.as_deref());
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

    /// Ctrl+N: новое окно. Отдельный процесс — проще, чем второе окно в том же.
    fn new_window(&self) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
    }

    fn open_dialog(&self) {
        if !self.maybe_save() {
            return;
        }
        if let Some(path) = choose(&self.window, FileChooserAction::Open, "Открыть") {
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
                "Файл удалён с диска.\n\nСохранить заново — записать документ по прежнему пути.",
                "_Сохранить заново",
                "_Оставить как есть",
            )
        } else {
            ask_dialog(
                &self.window,
                "Файл изменён на диске.\n\nПеречитать — несохранённые правки будут потеряны.\nОставить моё — записать правки поверх чужой версии.",
                "_Перечитать",
                "_Оставить моё",
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
        "В документе есть несохранённые изменения.",
    );
    dialog.add_button("_Отмена", ResponseType::Cancel);
    dialog.add_button("_Не сохранять", ResponseType::No);
    dialog.add_button("_Сохранить", ResponseType::Yes);
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
    const KEYS: &str = "\
Файлы
Ctrl+O — открыть
Ctrl+S — сохранить
Ctrl+Shift+S — сохранить как
Ctrl+N — новое окно

Правка
Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y — отменить и вернуть
Ctrl+B — жирный
Ctrl+I — курсив
Ctrl+Shift+X — зачёркнутый
Ctrl+K — ссылка

Поиск
Ctrl+F — поиск
Ctrl+H — поиск и замена
Enter / Ctrl+G — дальше, Shift+Enter — назад, Esc — закрыть

Вид
Ctrl+= / Ctrl+- / Ctrl+0 — размер текста
Ctrl+колесо — то же
Ctrl+Shift+C — копировать как HTML

Полный экран — хоткей Omarchy, не приложения.";
    let dialog = MessageDialog::new(
        Some(parent),
        DialogFlags::MODAL,
        MessageType::Info,
        ButtonsType::None,
        KEYS,
    );
    dialog.set_title("Клавиши MarkCraft");
    dialog.add_button("_Закрыть", ResponseType::Close);
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
    dialog.add_button("_Закрыть", ResponseType::Close);
    dialog.set_default_response(ResponseType::Close);
    dialog.run();
    dialog.close();
}

fn build_window(app: &Application, initial: Option<PathBuf>) {
    let (width, height) = geometry::read().unwrap_or((980, 760));
    let window = ApplicationWindow::builder()
        .application(app)
        .title("MarkCraft")
        .default_width(width)
        .default_height(height)
        .build();
    window.set_size_request(720, 520);

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
    });

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
            "openUrl" => {
                let url = message["url"].as_str().unwrap_or_default();
                if !url.is_empty() {
                    let _ = gtk::show_uri_on_window(Some(&sender.window), url, 0);
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

    window.add(&webview);
    webview.load_html(UI_HTML, base.as_deref());
    window.show_all();
    webview.grab_focus();
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

    let initial = args
        .into_iter()
        .find(|a| !a.starts_with('-'))
        .map(PathBuf::from);

    let app = Application::builder()
        .application_id("app.markcraft.MarkCraft")
        .build();
    app.connect_activate(move |app| build_window(app, initial.clone()));
    // Аргументы разобраны выше; GTK не должен пытаться открыть файл сам.
    app.run_with_args::<&str>(&[]);
}
