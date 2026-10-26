/// Какое действие соответствует нажатию. Разбор идёт по `event.code` —
/// физической клавише, — потому что на русской раскладке Ctrl+S даёт
/// `event.key === 'ы'`, и сравнение с 's' не срабатывает.
///
/// Возвращает имя действия или null. Ничего не делает и ничего не знает
/// про редактор: поэтому проверяется тестом без окна.
export function actionFor(event) {
  if (!event.ctrlKey && !event.metaKey) return null

  // Сначала физическая клавиша: на русской раскладке Ctrl+S даёт key='ы',
  // и только `code` остаётся латинским. Если код незнакомый или пустой
  // (программные события, некоторые экранные клавиатуры), пробуем `key`.
  const shift = !!event.shiftKey
  return byCode(event.code, shift) ?? byCode(codeFromKey(event.key), shift)
}

function byCode(code, shift = false) {
  switch (code) {
    case 'KeyS':
      return shift ? 'saveAs' : 'save'
    case 'KeyO':
      return 'open'
    case 'KeyN':
      return 'newWindow'
    case 'KeyV':
      return 'paste'
    case 'KeyB':
      return 'bold'
    case 'KeyI':
      return 'italic'
    case 'KeyK':
      return 'link'
    case 'KeyX':
      return shift ? 'strike' : null
    case 'KeyZ':
      return shift ? 'redo' : 'undo'
    case 'KeyY':
      return 'redo'
    case 'KeyF':
      return 'find'
    case 'KeyH':
      return 'replace'
    case 'KeyC':
      return shift ? 'copyHtml' : null
    case 'Slash':
      return 'help'
    case 'Equal':
    case 'NumpadAdd':
      return 'zoomIn'
    case 'Minus':
    case 'NumpadSubtract':
      return 'zoomOut'
    case 'Digit0':
    case 'Numpad0':
      return 'zoomReset'
    default:
      return null
  }
}

/// Латиница → код клавиши. Кириллица намеренно не разбирается: у неё нет
/// однозначного соответствия, и полагаться на неё нельзя.
function codeFromKey(key) {
  if (!key || key.length !== 1) return ''
  const upper = key.toUpperCase()
  if (upper >= 'A' && upper <= 'Z') return `Key${upper}`
  if (upper >= '0' && upper <= '9') return `Digit${upper}`
  if (key === '/') return 'Slash'
  if (key === '=' || key === '+') return 'Equal'
  if (key === '-') return 'Minus'
  return ''
}
