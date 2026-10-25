# Типографика Inkdown → MarkCraft

Числа сняты с `inkdown/src/renderer/src/styles/editor.css` и `Editor.tsx`  
(версия 2.0.2, коммит master на 2025-06). Это контракт: менять число можно только  
вместе с повторным измерением по скриншоту.

## База

| Параметр              | Значение                                                                          |
| --------------------- | --------------------------------------------------------------------------------- |
| база                  | 16 px                                                                             |
| line-height документа | 1.53                                                                              |
| колонка               | max-width 796 px, padding-inline 48 px → **700 px текста**                        |
| шрифт                 | системный UI sans (`system-ui, -apple-system, sans-serif`), моно — системный mono |
| свет: фон / текст     | `#ffffff` / `rgba(45,45,40,1)`                                                    |
| тьма: фон / текст     | `#1a1b1b` / `rgba(255,255,255,0.93)`                                              |

## Блоки

| Элемент     | Размер                                                              | Вес | Отступы                                                                         |
| ----------- | ------------------------------------------------------------------- | --- | ------------------------------------------------------------------------------- |
| h1          | 1.875em (30 px)                                                     | 700 | line-height 1.3, padding 3px 0, margin-top 1.2em, margin-bottom 3px             |
| h2          | 1.5em (24 px)                                                       | 600 | то же, margin-top 1em                                                           |
| h3          | 1.4em (22.4 px)                                                     | 600 | то же, margin-top 0.8em                                                         |
| h4          | 1.3em (20.8 px)                                                     | 500 | line-height 1.2, margin-top 0.7em                                               |
| h5, h6      | 1em                                                                 | 600 | как h4                                                                          |
| p           | 1em                                                                 | 400 | padding 3px 0, между абзацами margin-bottom 0.3em                               |
| blockquote  | 0.96em, italic                                                      | 400 | padding 3px 14px 3px 21px, полоса 4px слева, margin 0.2em/0.5em                 |
| ul / ol     | 1em                                                                 | 400 | padding-left 24px, padding-block 3px, item padding-block 5px                    |
| вложенность | ul: disc → circle → revert; ol: decimal → lower-alpha → upper-roman |     |                                                                                 |
| table       | 0.9em                                                               |     | рамки 1px, th bg 30% серого, ячейка .375em 1em .375em .75em, line-height 1.7    |
| code block  | 0.875em (14 px), mono                                               |     | фон `#2a2929` (свет) / `#1c1c1c` (тьма), скролл по X                            |
| inline code | 0.85em, mono                                                        |     | padding 2px 5px, radius 4px, фон `rgba(23,23,23,.07)` / `rgba(255,255,255,.05)` |
| hr          | 1px                                                                 |     | margin 2em 0                                                                    |
| img         | max-width 100%                                                      |     | radius 4px, margin 1em 0                                                        |

## Правила

1. Все размеры внутри документа — в `em`, чтобы Ctrl+= масштабировал целиком.
2. Никаких бандл-шрифтов. Шрифт берётся из системы, поэтому на Omarchy он совпадает с тем, что видит Inkdown.
3. Палитра не из этой таблицы, а из `colors.toml` (см. SPEC I5). Здесь только метрики.
4. Цвета структурных линий (таблицы, hr, цитата) выводятся из `foreground` темы с прозрачностью 20–30%.
5. Меняешь число — приложи скриншот до/после при 100% масштабе.