/// Адрес ссылки, внутри которой стоит позиция: `[текст](url "t")`, `<url>`
/// или голый `https://…`. Вынесено из редактора, чтобы проверялось тестом:
/// ошибка здесь означает «клик открыл не то».
export function linkHrefAt(text, pos) {
  const lineStart = text.lastIndexOf('\n', pos) + 1
  const lineEnd = text.indexOf('\n', pos)
  const line = text.slice(lineStart, lineEnd < 0 ? text.length : lineEnd)
  const at = pos - lineStart

  const inside = (start, length) => at >= start && at <= start + length

  const bracket = /\[([^\]]*)\]\(([^)\s]+)/g
  let m
  while ((m = bracket.exec(line))) {
    if (inside(m.index, m[0].length)) return m[2]
  }

  const autolink = /<((?:https?|mailto):[^>]+)>/g
  while ((m = autolink.exec(line))) {
    if (inside(m.index, m[0].length)) return m[1]
  }

  const bare = /https?:\/\/[^\s)<>]+/g
  while ((m = bare.exec(line))) {
    if (inside(m.index, m[0].length)) return m[0]
  }

  return null
}
