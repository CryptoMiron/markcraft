/// Тело блока кода: узлы `CodeText` идут без ограды и без отступа
/// отступного кода. Возвращает `{ text, first, last }` или null для пустого.
export function codeBody(doc, node) {
  let text = ''
  let first = -1
  let last = -1
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (child.name !== 'CodeText') continue
    text += doc.sliceString(child.from, child.to)
    if (first < 0) first = doc.lineAt(child.from).number
    last = doc.lineAt(Math.max(child.from, child.to - 1)).number
  }
  if (first < 0) return null
  return {
    // Незакрытая ограда тянет за собой хвостовой перевод строки; в буфере
    // он не нужен, а закрытая ограда его и не даёт.
    text: text.replace(/\n$/, ''),
    first,
    last,
  }
}
