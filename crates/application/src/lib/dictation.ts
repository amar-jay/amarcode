/** Pure helpers for inserting dictated text into the composer draft. */

export interface MergedDictation {
  value: string;
  cursor: number;
}

/**
 * Insert `transcript` at the selection in `value`, adding a separating
 * space only where the surrounding text does not already provide one.
 */
export function mergeDictatedText(
  value: string,
  selectionStart: number,
  selectionEnd: number,
  transcript: string,
): MergedDictation {
  const text = transcript.trim();
  if (!text) return { value, cursor: selectionStart };
  const start = Math.max(0, Math.min(selectionStart, value.length));
  const end = Math.max(start, Math.min(selectionEnd, value.length));
  const before = value.slice(0, start);
  const after = value.slice(end);
  const prefix = before && !/\s$/.test(before) ? " " : "";
  const suffix = after && !/^\s/.test(after) ? " " : "";
  const inserted = `${prefix}${text}`;
  return {
    value: `${before}${inserted}${suffix}${after}`,
    cursor: before.length + inserted.length,
  };
}

export function formatDictationTime(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
