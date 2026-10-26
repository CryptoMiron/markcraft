/// Шапка файла (frontmatter) — YAML между `---` в самом начале документа.
///
/// Разбирать её Markdown-парсером нельзя: он видит `---` как подчёркивание
/// setext-заголовка и прячет закрывающую черту вместе с текстом шапки.
/// Поэтому блок распознаём сами и рисуем как код.

/// Границы шапки: `{ from, to, first, last }` или null, если её нет.
/// `first`/`last` — номера строк с открывающей и закрывающей чертой.
export function findFrontmatter(doc) {
  if (doc.lines < 2) return null

  const open = doc.line(1)
  if (open.from !== 0 || open.text.trim() !== '---') return null

  let sawYaml = false
  for (let n = 2; n <= doc.lines; n++) {
    const line = doc.line(n)
    const text = line.text.trim()
    if (text === '') continue
    if (text === '---' || text === '...') {
      return { from: 0, to: line.to, first: 1, last: n }
    }
    // Первая содержательная строка должна быть парой `ключ: значение`,
    // иначе это обычный текст, а `---` — горизонтальная черта.
    if (!sawYaml) {
      if (!/^[A-Za-z_][\w-]*\s*:/.test(text)) return null
      sawYaml = true
    }
  }
  return null
}
