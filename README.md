# MarkCraft

Markdown-редактор для Omarchy: файл — источник истины, Omarchy-палитра,
типографика Inkdown. Один бинарник на системном WebKitGTK.

## Документы

- [`docs/SPEC.md`](docs/SPEC.md) — единственный список требований, R1.1…R5.6
- [`docs/TYPOGRAPHY.md`](docs/TYPOGRAPHY.md) — числа вёрстки, снятые с Inkdown
- [`docs/PROCESS.md`](docs/PROCESS.md) — как ведётся разработка
- [`docs/CUTS.md`](docs/CUTS.md) — что не делаем и почему
- [`docs/DECISIONS.md`](docs/DECISIONS.md) — журнал решений

## Срезы

| Срез | Состояние |
|---|---|
| S0 Каркас | готов — окно на нативном Wayland |
| S1 Файлы | готов — байтовая верность, автосейв, черновик, диалоги |
| S1b Внешние изменения | готов — опрос файла, диалоги, удаление |
| S2 Рендер | готов |
| S2b Подсветка кода | готов |
| S3 Правка | готов — кроме живого IME |
| S4 Omarchy | готов |
| S5 Клавиши | не начат |

## Сборка

```sh
bin/bundle          # ui/*.js|css → ui/built.html (нужен bun)
cargo build --release
./target/release/markcraft fixtures/gfm-full.md
bin/check
```

`ui/built.html` коммитится: `cargo build` работает без bun. После правки
`ui/*.js|css` обязателен `bin/bundle`, иначе `bin/check` упадёт.

Пакет: `cd pkgbuild && makepkg -f`.

## Проверка перед готовым срезом

1. `bin/check` зелёный
2. каждая фикстура открыта и сохранена без правок: `cmp` чист
3. чек-лист SPEC для среза пройден руками
