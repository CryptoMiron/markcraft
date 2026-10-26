# S1 — Файлы

Выход среза: `markcraft file.md` показывает текст файла; Ctrl+S пишет его обратно
байт-в-байт; закрытие с несохранёнными правками спрашивает.

Пока S1 не закрыт, в UI намеренно нет рендера: `<pre>` — это честно, а не «недоделанный
markdown». Так байтовая верность проверяется до того, как появится что портить.
## Шаги

1. `src/doc.rs` — `Doc { path, text, dirty }`, чтение/запись **байтами**,
   никаких `String::from_utf8_lossy` на пути сохранения.
2. `src/main.rs` — загрузка файла в webview, IPC `save` / `loaded`.
3. `src/main.rs` — Ctrl+S, Ctrl+Shift+S (портал через `ashpd`), Ctrl+O.
4. Заголовок окна со звёздочкой, диалог несохранённых изменений.
5. `tests/roundtrip.rs` — фикстуры туда-обратно, `assert_eq!` байтов.
## Проверка

```sh
cargo test roundtrip
for f in fixtures/*.md; do
  cp "$f" /tmp/a.md && ./target/debug/markcraft /tmp/a.md   # открыть, Ctrl+S, закрыть
  cmp "$f" /tmp/a.md || echo "ПОРЧА: $f"
done
```
