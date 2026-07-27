export type SearchHighlightSegment = {
  text: string;
  highlighted: boolean;
};

type LiteralMatch = {
  start: number;
  end: number;
};

export function buildSearchHighlightSegments(
  text: string,
  query: string,
  maximumCharacters: number = 10,
): SearchHighlightSegment[] {
  if (!query) return segment(text, false);

  const match = findCaseInsensitiveLiteral(text, query);
  if (!match) return segment(text, false);

  const characters = Array.from(text);
  const matchStart = Array.from(text.slice(0, match.start)).length;
  const matchLength = Array.from(text.slice(match.start, match.end)).length;
  const snippetLength = Math.max(maximumCharacters, matchLength);
  let start = Math.max(0, matchStart - Math.floor((snippetLength - matchLength) / 2));
  let end = Math.min(characters.length, start + snippetLength);
  start = Math.max(0, end - snippetLength);
  const matchEnd = matchStart + matchLength;

  return [
    ...segment(
      start > 0
        ? `...${characters.slice(start, matchStart).join('')}`
        : characters.slice(0, matchStart).join(''),
      false,
    ),
    ...segment(characters.slice(matchStart, matchEnd).join(''), true),
    ...segment(
      `${characters.slice(matchEnd, end).join('')}${end < characters.length ? '...' : ''}`,
      false,
    ),
  ];
}

function findCaseInsensitiveLiteral(text: string, query: string): LiteralMatch | null {
  const foldedQuery = foldForSearch(query);
  if (!foldedQuery) return null;

  let foldedText = '';
  const originalStarts: number[] = [];
  const originalEnds: number[] = [];
  let originalOffset = 0;
  for (const character of text) {
    const start = originalOffset;
    originalOffset += character.length;
    const foldedCharacter = character.toLowerCase();
    foldedText += foldedCharacter;
    for (let index = 0; index < foldedCharacter.length; index += 1) {
      originalStarts.push(start);
      originalEnds.push(originalOffset);
    }
  }

  const foldedStart = foldedText.indexOf(foldedQuery);
  if (foldedStart < 0) return null;
  const foldedEnd = foldedStart + foldedQuery.length - 1;
  return {
    start: originalStarts[foldedStart] ?? 0,
    end: originalEnds[foldedEnd] ?? text.length,
  };
}

function foldForSearch(text: string): string {
  return Array.from(text, (character) => character.toLowerCase()).join('');
}

function segment(text: string, highlighted: boolean): SearchHighlightSegment[] {
  return text ? [{ text, highlighted }] : [];
}
