// Fail-closed selectors for the Paint example. Always use a fresh UIA snapshot.
const aliases = {
  editColors: ['Renkleri düzenle', 'Edit colors', 'Edit colours'],
  hex: ['RGB onaltılık', 'RGB hexadecimal', 'Hex', 'Hexadecimal', 'Hex color', 'Hex colour'],
  confirm: ['Tamam', 'OK'],
};
const roles = {editColors: 'Button', hex: 'Edit', confirm: 'Button'};
const normalized = value => value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLowerCase();

export function validateSelectors(overrides = {}) {
  if (!overrides || Array.isArray(overrides) || typeof overrides !== 'object') throw new Error('Selectors must be an object');
  for (const [key, value] of Object.entries(overrides)) {
    if (!Object.hasOwn(roles, key) || !Array.isArray(value) || !value.length || value.some(v => typeof v !== 'string' || !v.trim())) {
      throw new Error(`Invalid Paint selector: ${key}; expected a non-empty array of names`);
    }
  }
  return overrides;
}

export function paintNodes(tree) {
  return tree.split('\n').flatMap(line => {
    const match = line.match(/^\s*\[(\d+)\]\s+(\w+)(?:\s+"([^"]*)")?/);
    if (!match || /\s(?:offscreen|disabled)(?:\s|$)/.test(line)) return [];
    const value = line.match(/\bvalue=("(?:\\.|[^"\\])*")/);
    return [{index: Number(match[1]), role: match[2], name: match[3] ?? '', value: value ? JSON.parse(value[1]) : '', line}];
  });
}

export function paintControl(tree, key, overrides = {}) {
  validateSelectors(overrides);
  if (!Object.hasOwn(roles, key)) throw new Error(`Unknown Paint control: ${key}`);
  const names = (overrides[key] ?? aliases[key]).map(normalized);
  const matches = paintNodes(tree).filter(node => node.role === roles[key] && names.includes(normalized(node.name)));
  if (matches.length !== 1) throw new Error(`Expected one visible Paint ${key} control; found ${matches.length}. Supply --selectors with names from the current UIA tree.`);
  return matches[0];
}

export function verifyPaintSetup(tree, plan) {
  const [width, height] = plan.canvas;
  if (![width, height, plan.pen_size].every(v => Number.isSafeInteger(v) && v > 0)) throw new Error('Invalid plan dimensions or pen size');
  const dimensions = new RegExp(`\\b${width}\\s*[×x]\\s*${height}\\s*(?:px|pixels?|piksel)\\b`, 'i');
  const nodes = paintNodes(tree);
  if (!nodes.some(node => dimensions.test(node.line))) throw new Error('Canvas dimensions do not match plan');
  const size = new RegExp(`^${plan.pen_size}\\s*(?:pixels?|piksel|px)$`, 'i');
  if (!nodes.some(node => node.role === 'Slider' && size.test(node.value))) throw new Error('Set the pencil size before drawing; expected a pixel-valued size slider');
}
