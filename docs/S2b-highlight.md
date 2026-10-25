# S2b — Подсветка кода

Выход среза: блоки кода подсвечены, тема одна, ноль сетевых запросов.

**Готово 2026-10-25.**

## Что сделано

13 языков через `@codemirror/legacy-modes` — это уже готовые stream-парсеры,
которые не тянут по грамматике на язык:

rust, python, javascript/typescript, json, shell, yaml, toml, sql, go,
c, c++, css, xml/html.

Блок без языка (```` ``` ````) остаётся просто моноширинным — так и должно быть.

Цвета — CSS-переменные `--mc-tok-*`, поэтому светлая и тёмная темы красят код
без пересборки бандла. Правила скоуповые: `.cm-line.mc-code .tok-*`, иначе
подсветка доставалась бы markdown-маркерам (`#`, `**`).

Цена: `built.html` 516 → 655 KB, бинарник 959 KB → 1.1 MB.

Блок кода тёмный в обеих темах (как в Inkdown), поэтому и гамма токенов одна —
иначе на светлой теме тёмные цвета не читались бы на тёмном фоне. Проверено
скриншотом с `GTK_THEME=Adwaita:light`.

## Ловушка, на которую ушёл час

`LanguageDescription.of({ support: language })` молча не работает: `lang-markdown`
ждёт **`LanguageSupport`**, а не `StreamLanguage`. Со `support: language`
получается `TypeError: undefined is not an object (evaluating
'found.support.language.parser')` — и весь документ остаётся одним узлом
`Document`, без разметки вообще. Правильно:

```js
const support = new LanguageSupport(StreamLanguage.define(mode))
LanguageDescription.of({ name, alias, support, load: async () => support.language })
```

Симптом коварный: ошибки в консоли нет, просто ничего не подсвечено.

## Как проверять

```sh
./bin/bundle && cargo build --release
./target/release/markcraft fixtures/code-langs.md
```

Считать токены, а не смотреть глазами:

```js
document.querySelectorAll('[class^=tok-]').length
```

Ноль означает «парсер не подключился», а не «тема бледная».
