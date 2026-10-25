mod doc;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{
    Application, ApplicationWindow, ButtonsType, DialogFlags, FileChooserAction, FileChooserNative,
    MessageDialog, MessageType, ResponseType,
};
use javascriptcore::ValueExt;
use webkit2gtk::{LoadEvent, UserContentManager, UserContentManagerExt, WebView, WebViewExt};

use doc::{Doc, draft, geometry};

const UI_HTML: &str = include_str!("../ui/index.html");
const AUTOSAVE: Duration = Duration::from_millis(1200);
const DRAFT_EVERY: Duration = Duration::from_millis(300);

struct App {
    window: ApplicationWindow,
    webview: WebView,
    doc: RefCell<Doc>,
    dirty: Cell<bool>,
    /// Автосейв уже запланирован: второй таймер не нужен, первый возьмёт свежий текст.
    pending: Cell<bool>,
    /// Восстановленный черновик не пишется в файл, пока пользователь не сохранит сам.
    recovered: Cell<bool>,
    /// Когда черновик писался в последний раз: на каждую букву — слишком часто.
    last_draft: Cell<Option<Instant>>,
    /// Открыт вопрос «сохранить?»: пока пользователь не решил, в файл не пишем.
    asking: Cell<bool>,
    /// Что показать, когда страница догрузилась: черновик важнее файла из argv.
    restore: RefCell<Option<draft::Draft>>,
    initial: Option<PathBuf>,
}

impl App {
    fn refresh_title(&self) {
        let star = if self.dirty.get() { "* " } else { "" };
        self.window
            .set_title(&format!("{star}{} — MarkCraft", self.doc.borrow().name()));
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
        // Черновик — на каждые 300 мс правки, чтобы kill -9 не съел набранное.
        let now = Instant::now();
        if self
            .last_draft
            .get()
            .is_none_or(|t| now.duration_since(t) > DRAFT_EVERY)
        {
            self.last_draft.set(Some(now));
            let _ = draft::write(&self.doc.borrow().text, self.doc.borrow().path());
        }
        self.schedule_save();
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
            if later.doc.borrow().path().is_some() && !later.recovered.get() && !later.asking.get()
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
        if self.doc.borrow().path().is_none() {
            let Some(path) = choose(&self.window, FileChooserAction::Save, "Сохранить как")
            else {
                return false;
            };
            self.doc.borrow_mut().set_path(with_md(path));
        }
        if let Err(e) = self.doc.borrow_mut().save() {
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
                *self.doc.borrow_mut() = doc;
                self.push();
            }
            Err(e) => error(&self.window, &format!("{}: {e}", path.display())),
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
        last_draft: Cell::new(None),
        restore: RefCell::new(draft::take()),
        initial,
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

    window.add(&webview);
    webview.load_html(UI_HTML, None);
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
