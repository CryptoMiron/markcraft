use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Перевод строки, которым файл был записан. Документ в памяти всегда LF.
#[derive(Clone, Copy, PartialEq)]
enum Eol {
    Lf,
    Crlf,
}

/// Отпечаток файла на диске: время правки и размер. Хватает, чтобы заметить
/// чужую запись, и не требует ни inotify, ни прав на каталог.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Stamp {
    mtime: Option<SystemTime>,
    len: u64,
}

/// Что стало с файлом с тех пор, как мы его читали или писали.
pub enum Disk {
    Same,
    Modified,
    Gone,
}

pub struct Doc {
    path: Option<PathBuf>,
    /// Текст без BOM, с LF. Это то, что видит редактор.
    pub text: String,
    /// Копия на момент последней записи, для «есть несохранённое».
    saved: String,
    eol: Eol,
    bom: bool,
    /// Отпечаток того, что мы сами читали или писали последним.
    stamp: Option<Stamp>,
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp {
        mtime: meta.modified().ok(),
        len: meta.len(),
    })
}

impl Doc {
    pub fn empty() -> Self {
        Self {
            path: None,
            text: String::new(),
            saved: String::new(),
            eol: Eol::Lf,
            bom: false,
            stamp: None,
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
        let body = if bom { &bytes[3..] } else { &bytes[..] };
        // Не UTF-8 — честная ошибка: молча перекодировать чужой файл нельзя.
        let raw = String::from_utf8(body.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "файл не в кодировке UTF-8"))?;

        let crlf = raw.matches("\r\n").count();
        let lf = raw.matches('\n').count() - crlf;
        let eol = if crlf > lf { Eol::Crlf } else { Eol::Lf };
        let text = if eol == Eol::Crlf {
            raw.replace("\r\n", "\n")
        } else {
            raw
        };

        Ok(Self {
            path: Some(path.to_owned()),
            saved: text.clone(),
            text,
            eol,
            bom,
            stamp: stamp_of(path),
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// Возвращает true, если текст разошёлся с тем, что лежит на диске.
    pub fn set_text(&mut self, text: &str) -> bool {
        self.text.clear();
        self.text.push_str(text);
        self.text != self.saved
    }

    pub fn name(&self) -> String {
        self.path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Без имени".to_owned())
    }

    /// Байты ровно в том виде, в каком файл лежал на диске.
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.text.len() + 3);
        if self.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        match self.eol {
            Eol::Lf => out.extend_from_slice(self.text.as_bytes()),
            Eol::Crlf => out.extend_from_slice(self.text.replace('\n', "\r\n").as_bytes()),
        }
        out
    }

    /// Пишет через временный файл рядом: падение посреди записи не оставит
    /// обрезанный документ, а права исходного файла сохраняются.
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "у документа нет пути"))?;

        // Права уважаются: rename обошёл бы read-only, но это был бы чужой файл.
        if fs::metadata(path).is_ok_and(|m| m.permissions().readonly()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "файл доступен только для чтения",
            ));
        }

        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".markcraft-tmp");
        let tmp = PathBuf::from(tmp);

        fs::write(&tmp, self.bytes())?;
        if let Ok(meta) = fs::metadata(path) {
            let _ = fs::set_permissions(&tmp, meta.permissions());
        }
        if let Err(e) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        self.saved = self.text.clone();
        self.stamp = stamp_of(path);
        Ok(())
    }

    pub fn disk_state(&self) -> Disk {
        let Some(path) = self.path() else {
            return Disk::Same;
        };
        match stamp_of(path) {
            None => Disk::Gone,
            Some(stamp) if Some(stamp) == self.stamp => Disk::Same,
            Some(_) => Disk::Modified,
        }
    }

    /// «Оставить моё»: чужая запись принята как текущее состояние диска,
    /// чтобы не спрашивать об одном и том же файле повторно.
    pub fn accept_disk(&mut self) {
        if let Some(path) = self.path.clone() {
            self.stamp = stamp_of(&path);
        }
    }
}

/// Черновик, который переживает kill -9. Лежит рядом с чужими данными не должен:
/// путь — `$XDG_STATE_HOME/markcraft/draft`, содержимое — JSON.
pub mod draft {
    use super::*;

    pub struct Draft {
        pub text: String,
        /// Файл, из которого черновик вырос, если он был.
        pub path: Option<PathBuf>,
    }

    fn dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(base.join("markcraft"))
    }

    fn file() -> Option<PathBuf> {
        dir().map(|d| d.join("draft"))
    }

    pub fn write(text: &str, path: Option<&Path>) -> io::Result<()> {
        let Some(file) = file() else { return Ok(()) };
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let payload = serde_json::json!({
            "path": path.map(|p| p.to_string_lossy().into_owned()),
            "text": text,
        });
        fs::write(file, payload.to_string())
    }

    pub fn take() -> Option<Draft> {
        let file = file()?;
        let raw = fs::read_to_string(&file).ok()?;
        let _ = fs::remove_file(&file);
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        Some(Draft {
            text: value["text"].as_str()?.to_owned(),
            path: value["path"].as_str().map(PathBuf::from),
        })
    }

    pub fn clear() {
        if let Some(file) = file() {
            let _ = fs::remove_file(file);
        }
    }
}

/// Размер окна между запусками. Позиция не сохраняется: на Wayland её
/// назначает композитор, а в тайлинге она и не нужна.
pub mod geometry {
    use super::*;

    fn file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
        Some(base.join("markcraft").join("window"))
    }

    pub fn read() -> Option<(i32, i32)> {
        let raw = fs::read_to_string(file()?).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        Some((
            value["width"].as_i64()? as i32,
            value["height"].as_i64()? as i32,
        ))
    }

    pub fn write(width: i32, height: i32) {
        let Some(file) = file() else { return };
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let payload = serde_json::json!({ "width": width, "height": height });
        let _ = fs::write(file, payload.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> Vec<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        files.sort();
        assert!(files.len() >= 10, "фикстуры потерялись");
        files
    }

    #[test]
    fn roundtrip_is_byte_exact() {
        for path in fixtures() {
            let original = fs::read(&path).unwrap();
            let doc = Doc::open(&path).unwrap();
            assert_eq!(
                doc.bytes(),
                original,
                "порча при открытии/закрытии: {}",
                path.display()
            );
        }
    }

    #[test]
    fn save_writes_same_bytes() {
        let dir = std::env::temp_dir().join(format!("markcraft-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        for path in fixtures() {
            let copy = dir.join(path.file_name().unwrap());
            fs::copy(&path, &copy).unwrap();

            let mut doc = Doc::open(&copy).unwrap();
            doc.text.push('x');
            doc.save().unwrap();
            assert_eq!(
                fs::read(&copy).unwrap(),
                doc.bytes(),
                "запись не совпала с буфером: {}",
                copy.display()
            );

            // Переоткрытие не меняет документ: BOM и CRLF не удваиваются.
            let again = Doc::open(&copy).unwrap();
            assert_eq!(again.text, doc.text);
            assert_eq!(again.bytes(), doc.bytes());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn crlf_stays_crlf() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/crlf.md");
        let doc = Doc::open(&path).unwrap();
        assert!(doc.text.starts_with("# CRLF\n\n"));
        assert!(doc.bytes().windows(2).any(|w| w == b"\r\n"));
        assert!(!doc.bytes().windows(2).any(|w| w == b"\n\n"));
    }

    #[test]
    fn bom_is_kept() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/bom-utf8.md");
        let doc = Doc::open(&path).unwrap();
        assert!(doc.text.starts_with("# С BOM"));
        assert!(doc.bytes().starts_with(&[0xEF, 0xBB, 0xBF]));
    }

    #[test]
    fn disk_state_sees_outside_writes() {
        let dir = std::env::temp_dir().join(format!("markcraft-disk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.md");
        fs::write(&path, "# раз\n").unwrap();

        let mut doc = Doc::open(&path).unwrap();
        assert!(matches!(doc.disk_state(), Disk::Same));

        // Размер и mtime сдвигаются: чужая запись видна.
        fs::write(&path, "# раз\nдва\n").unwrap();
        assert!(matches!(doc.disk_state(), Disk::Modified));

        doc.accept_disk();
        assert!(matches!(doc.disk_state(), Disk::Same));

        // Своя запись тоже не считается чужой.
        doc.text.push_str("три\n");
        doc.save().unwrap();
        assert!(matches!(doc.disk_state(), Disk::Same));

        fs::remove_file(&path).unwrap();
        assert!(matches!(doc.disk_state(), Disk::Gone));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn draft_survives_and_is_taken_once() {
        // Отдельный XDG_STATE_HOME, чтобы не трогать настоящий черновик.
        let dir = std::env::temp_dir().join(format!("markcraft-draft-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("XDG_STATE_HOME", &dir) };

        draft::write("черновик\nстрока", Some(Path::new("/tmp/a.md"))).unwrap();
        let taken = draft::take().expect("черновик не прочитался");
        assert_eq!(taken.text, "черновик\nстрока");
        assert_eq!(taken.path.as_deref(), Some(Path::new("/tmp/a.md")));
        assert!(
            draft::take().is_none(),
            "черновик не должен читаться дважды"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
