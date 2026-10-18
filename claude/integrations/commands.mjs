/** Recognize ordinary direct commands only; this is not a shell interpreter. */
export function commands(event, mode = 'direct') {
  if (Array.isArray(event?.argv) && event.argv.every((word) => typeof word === 'string')) return [event.argv];
  if (typeof event?.command !== 'string' || mode !== 'review' && event.command.length > 65536) return [];
  const result = []; let words = [], word = '', quote = '', escaped = false, started = false;
  const token = () => { if (word || mode === 'review' && started) words.push(word); word = ''; started = false; };
  const command = () => { token(); if (words.length) result.push(words); words = []; };
  for (const character of event.command) {
    if (escaped) { word += character; escaped = false; continue; }
    if (mode !== 'review' && character === '\\' && quote !== "'") { escaped = true; continue; }
    if (quote) { if (character === quote) quote = ''; else { word += character; started = true; } continue; }
    if (character === "'" || character === '"') { quote = character; started = true; continue; }
    if ((';|&\n' + (mode === 'review' ? '\r' : '')).includes(character)) { command(); continue; }
    if (/\s/.test(character)) token(); else { word += character; started = true; }
  }
  if (mode !== 'review' && (quote || escaped)) return [];
  command(); return result;
}

