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

/// Открывается ли ссылка этим нажатием. Только Ctrl/Cmd + левая кнопка:
/// обычный клик обязан ставить курсор и начинать выделение, иначе в текст
/// ссылки нельзя попасть мышью.
export function opensLinkOn(event) {
  return event.button === 0 && Boolean(event.ctrlKey || event.metaKey)
}

/// Номер строки заголовка, на который указывает якорь `#slug`.
/// shortcut: дубликаты заголовков не нумеруются суффиксами, как на GitHub;
/// если понадобится — добавить счётчик одинаковых slug.
export function anchorLine(text, href) {
  let slug = href.replace(/^#/, '')
  try {
    slug = decodeURIComponent(slug)
  } catch {}
  slug = slug.toLowerCase()
  const lines = text.split('\n')
  for (let i = 0; i < lines.length; i++) {
    const heading = /^#{1,6}\s+(.+?)\s*#*\s*$/.exec(lines[i])
    if (!heading) continue
    const own = heading[1]
      .replace(/[*_`~]/g, '')
      .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\s-]/gu, '')
      .trim()
      .replace(/\s+/g, '-')
    if (own === slug) return i + 1
  }
  return null
}
