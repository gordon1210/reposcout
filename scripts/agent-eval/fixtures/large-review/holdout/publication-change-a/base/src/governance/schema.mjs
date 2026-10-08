import { object, text, integer, choice } from '../core/validation.mjs';
import { requireValue } from '../core/errors.mjs';

export const limits = Object.freeze({ depth: 8, nodes: 128, rules: 64, stages: 8, checklist: 24 });
export const factTypes = Object.freeze({
  title: 'text', language: 'text', authorId: 'text', tags: 'list',
  characters: 'number', words: 'number', lines: 'number', sectionCount: 'number',
  sectionTitles: 'list', linkCount: 'number', unresolvedMarkers: 'number',
  codeBlocks: 'number', emptySections: 'number', hasUnclosedFence: 'boolean',
});

export function exact(value, keys, label) {
  object(value, label);
  requireValue(Object.keys(value).every(key => keys.includes(key)), 'invalid_policy', `${label} has unknown fields`);
  return value;
}

function list(value, label, maximum, minimum = 0) {
  requireValue(Array.isArray(value) && value.length >= minimum && value.length <= maximum,
    'invalid_policy', `${label} must contain ${minimum}..${maximum} entries`);
  return value;
}

export function identifier(value, label) {
  text(value, label, { max: 80 });
  requireValue(/^[a-z][a-z0-9_-]*$/u.test(value), 'invalid_policy', `${label} must be a lowercase identifier`);
  return value;
}

function scalar(value, type) {
  if (type === 'text') return text(value, 'expression value', { max: 500, empty: true });
  if (type === 'number') return integer(value, 'expression value', 0, 1000000);
  requireValue(typeof value === 'boolean', 'invalid_policy', 'expression value must be boolean');
  return value;
}

export function validateExpression(expression) {
  let count = 0;
  function visit(node, depth) {
    requireValue(++count <= limits.nodes && depth <= limits.depth,
      'invalid_policy', 'rule expression exceeds its complexity budget');
    object(node, 'expression');
    const op = choice(node.op, ['all', 'any', 'not', 'eq', 'ne', 'gt', 'gte', 'lt', 'lte', 'contains', 'oneOf'], 'expression operator');
    if (op === 'all' || op === 'any') {
      exact(node, ['op', 'args'], 'logical expression');
      return { op, args: list(node.args, 'logical operands', 16, 1).map(arg => visit(arg, depth + 1)) };
    }
    if (op === 'not') {
      exact(node, ['op', 'arg'], 'negation');
      return { op, arg: visit(node.arg, depth + 1) };
    }
    exact(node, ['op', 'fact', 'value'], 'comparison');
    const fact = choice(node.fact, Object.keys(factTypes), 'fact');
    const type = factTypes[fact];
    if (['gt', 'gte', 'lt', 'lte'].includes(op)) {
      requireValue(type === 'number', 'invalid_policy', 'ordering requires a numeric fact');
    }
    if (op === 'contains') {
      requireValue(type === 'text' || type === 'list', 'invalid_policy', 'contains requires text or a list');
      return { op, fact, value: scalar(node.value, 'text') };
    }
    requireValue(type !== 'list', 'invalid_policy', 'list facts support only contains');
    if (op === 'oneOf') {
      return { op, fact, value: list(node.value, 'allowed values', 32, 1).map(value => scalar(value, type)) };
    }
    return { op, fact, value: scalar(node.value, type) };
  }
  return visit(expression, 0);
}

function unique(items, label) {
  requireValue(new Set(items.map(item => item.id)).size === items.length,
    'invalid_policy', `${label} ids must be unique`);
  return items;
}

export function validateDefinition(input) {
  exact(input, ['name', 'description', 'extendsVersionId', 'rules', 'removeRules', 'stages'], 'policy definition');
  const rules = unique(list(input.rules ?? [], 'rules', limits.rules).map(rule => {
    exact(rule, ['id', 'message', 'severity', 'when', 'require'], 'rule');
    return {
      id: identifier(rule.id, 'rule id'), message: text(rule.message, 'rule message', { max: 400 }),
      severity: choice(rule.severity ?? 'error', ['error', 'warning'], 'rule severity'),
      when: rule.when === undefined ? null : validateExpression(rule.when),
      require: validateExpression(rule.require),
    };
  }), 'rule');
  const removeRules = list(input.removeRules ?? [], 'removed rules', limits.rules)
    .map(value => identifier(value, 'removed rule id'));
  requireValue(new Set(removeRules).size === removeRules.length &&
    !rules.some(rule => removeRules.includes(rule.id)), 'invalid_policy', 'rule overrides conflict');
  const stages = input.stages === undefined ? null : unique(list(input.stages, 'stages', limits.stages, 1).map(stage => {
    exact(stage, ['id', 'name', 'distinctReviewer', 'checklist'], 'review stage');
    requireValue(stage.distinctReviewer === undefined || typeof stage.distinctReviewer === 'boolean',
      'invalid_policy', 'distinctReviewer must be boolean');
    return {
      id: identifier(stage.id, 'stage id'), name: text(stage.name, 'stage name', { max: 120 }),
      distinctReviewer: stage.distinctReviewer === true,
      checklist: unique(list(stage.checklist ?? [], 'checklist', limits.checklist).map(item => {
        exact(item, ['id', 'label', 'evidenceRequired'], 'checklist item');
        requireValue(item.evidenceRequired === undefined || typeof item.evidenceRequired === 'boolean',
          'invalid_policy', 'evidenceRequired must be boolean');
        return { id: identifier(item.id, 'checklist id'), label: text(item.label, 'checklist label', { max: 240 }),
          evidenceRequired: item.evidenceRequired === true };
      }), 'checklist'),
    };
  }), 'stage');
  return {
    name: text(input.name, 'policy name', { max: 160 }),
    description: text(input.description ?? '', 'policy description', { max: 2000, empty: true }),
    extendsVersionId: input.extendsVersionId === undefined || input.extendsVersionId === null ? null :
      text(input.extendsVersionId, 'parent policy version', { max: 100 }),
    rules, removeRules, stages,
  };
}
