const DARK_REFERENCE_PALETTE = [
  '#fca5a5',
  '#f9a8d4',
  '#fcd34d',
  '#bef264',
  '#86efac',
  '#5eead4',
  '#7dd3fc',
  '#93c5fd',
  '#a5b4fc',
  '#c4b5fd',
  '#f0abfc',
  '#fdba74',
];

const LIGHT_REFERENCE_PALETTE = [
  '#2563eb',
  '#dc2626',
  '#0891b2',
  '#ca8a04',
  '#16a34a',
  '#9333ea',
  '#db2777',
  '#0f766e',
  '#4f46e5',
  '#c2410c',
  '#be123c',
  '#15803d',
];

function hashString(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i++) {
    hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  }
  return hash;
}

function hslToHex(hue: number, saturation: number, lightness: number): string {
  const s = saturation / 100;
  const l = lightness / 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const h = hue / 60;
  const x = c * (1 - Math.abs((h % 2) - 1));

  let r = 0;
  let g = 0;
  let b = 0;

  if (h >= 0 && h < 1) {
    r = c; g = x;
  } else if (h >= 1 && h < 2) {
    r = x; g = c;
  } else if (h >= 2 && h < 3) {
    g = c; b = x;
  } else if (h >= 3 && h < 4) {
    g = x; b = c;
  } else if (h >= 4 && h < 5) {
    r = x; b = c;
  } else {
    r = c; b = x;
  }

  const m = l - c / 2;
  const toHex = (v: number) => Math.round((v + m) * 255).toString(16).padStart(2, '0');
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

function parseHexColor(color: string): [number, number, number] | null {
  const normalized = color.trim().replace(/^#/, '');
  if (!/^[0-9a-fA-F]+$/.test(normalized)) return null;

  if (normalized.length === 3) {
    const r = parseInt(normalized[0] + normalized[0], 16);
    const g = parseInt(normalized[1] + normalized[1], 16);
    const b = parseInt(normalized[2] + normalized[2], 16);
    return [r, g, b];
  }

  if (normalized.length === 6) {
    const r = parseInt(normalized.slice(0, 2), 16);
    const g = parseInt(normalized.slice(2, 4), 16);
    const b = parseInt(normalized.slice(4, 6), 16);
    return [r, g, b];
  }

  return null;
}

export function colorToRgba(color: string, alpha: number): string {
  const rgb = parseHexColor(color);
  if (!rgb) {
    return `rgba(99, 102, 241, ${alpha})`;
  }
  return `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${alpha})`;
}

export function createSeriesColorScale(seriesKeys: string[], darkTheme: boolean): (seriesKey: string, index: number) => string {
  const palette = darkTheme ? DARK_REFERENCE_PALETTE : LIGHT_REFERENCE_PALETTE;
  const chartSeed = hashString(seriesKeys.join('|'));
  const paletteOffset = chartSeed % palette.length;

  return (seriesKey: string, index: number) => {
    if (index < palette.length) {
      return palette[(index + paletteOffset) % palette.length];
    }

    const localSeed = hashString(`${seriesKey}:${index}:${chartSeed}`);
    const hue = (localSeed * 0.61803398875 + index * 137.507764) % 360;
    const saturation = darkTheme ? 72 : 70;
    const lightness = darkTheme ? 66 : 45;
    return hslToHex(hue, saturation, lightness);
  };
}
