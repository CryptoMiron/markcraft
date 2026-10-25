/// Правила правки, вынесенные из редактора: их можно проверить тестом,
/// не поднимая окно.

/// Что вставить на новой строке, если Enter нажат в конце `line`.
/// null — обычный перевод строки.
export function nextLinePrefix(line) {
  const bullet = /^(\s*)([-*+])(\s+)(\[[ xX]\]\s+)?/.exec(line)
  if (bullet) {
    const [, indent, mark, space, task] = bullet
    const rest = line.slice(bullet[0].length)
    // Пустой пункт: Enter выходит из списка, а не плодит маркеры.
    if (rest.trim() === '') return { clear: true }
    return { prefix: `${indent}${mark}${space}${task ? '[ ] ' : ''}` }
  }

  const ordered = /^(\s*)(\d+)([.)])(\s+)/.exec(line)
  if (ordered) {
    const [, indent, number, dot, space] = ordered
    const rest = line.slice(ordered[0].length)
    if (rest.trim() === '') return { clear: true }
    return { prefix: `${indent}${Number(number) + 1}${dot}${space}` }
  }

  const quote = /^(\s*)(>\s?)/.exec(line)
  if (quote) {
    const [, indent, mark] = quote
    const rest = line.slice(quote[0].length)
    if (rest.trim() === '') return { clear: true }
    return { prefix: `${indent}${mark}` }
  }

  return null
}

/// Оборачивает выделение маркером или снимает обёртку, если она уже есть.
/// Возвращает {from, to, insert} — готовую правку для CM, и {anchor, head} —
/// куда поставить курсор после неё.
export function toggleWrap(text, from, to, marker) {
  const len = marker.length
  const selected = text.slice(from, to)

  // Маркеры снаружи выделения: `**|текст|**`.
  if (text.slice(from - len, from) === marker && text.slice(to, to + len) === marker) {
    return {
      from: from - len,
      to: to + len,
      insert: selected,
      anchor: from - len,
      head: to - len,
    }
  }

  // Маркеры внутри выделения: `|**текст**|`.
  if (selected.length >= len * 2 && selected.startsWith(marker) && selected.endsWith(marker)) {
    const inner = selected.slice(len, -len)
    return { from, to, insert: inner, anchor: from, head: from + inner.length }
  }

  // Курсор внутри слова без выделения: оборачиваем слово целиком.
  if (from === to) {
    const word = wordAt(text, from)
    if (word) {
      return {
        from: word.from,
        to: word.to,
        insert: marker + text.slice(word.from, word.to) + marker,
        anchor: word.from + len,
        head: word.to + len,
      }
    }
    return { from, to, insert: marker + marker, anchor: from + len, head: from + len }
  }

  return {
    from,
    to,
    insert: marker + selected + marker,
    anchor: from + len,
    head: to + len,
  }
}

/// Слово под позицией: границы по пробелам и знакам, которые в markdown
/// разделяют разметку. null — позиция на границе слова.
export function wordAt(text, pos) {
  const isWord = (c) => c && !/[\s*_~`[\]()<>]/.test(c)
  if (!isWord(text[pos - 1]) && !isWord(text[pos])) return null
  let from = pos
  while (from > 0 && isWord(text[from - 1])) from--
  let to = pos
  while (to < text.length && isWord(text[to])) to++
  return { from, to }
}

/// Ссылка `[подпись](url)` вокруг позиции, если она там уже есть.
/// Нужна, чтобы Ctrl+K внутри ссылки правил адрес, а не плодил вложенные.
export function existingLink(text, pos) {
  const lineStart = text.lastIndexOf('\n', pos) + 1
  const lineEnd = text.indexOf('\n', pos)
  const line = text.slice(lineStart, lineEnd < 0 ? text.length : lineEnd)
  const at = pos - lineStart
  const re = /\[([^\]]*)\]\(([^)\s]*)\)/g
  let m
  while ((m = re.exec(line))) {
    const end = m.index + m[0].length
    if (at >= m.index && at <= end) {
      const urlFrom = lineStart + m.index + m[1].length + 3
      return { urlFrom, urlTo: urlFrom + m[2].length }
    }
  }
  return null
}

/// Ссылка для Ctrl+K: выделенный текст становится подписью, курсор — на адресе.
export function insertLink(text, from, to) {
  const label = text.slice(from, to) || 'текст'
  const insert = `[${label}](https://)`
  const urlStart = from + label.length + 3
  return { from, to, insert, anchor: urlStart, head: urlStart + 'https://'.length }
}
