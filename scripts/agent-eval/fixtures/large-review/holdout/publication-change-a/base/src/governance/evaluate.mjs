import { validateExpression } from './schema.mjs';

function evaluate(node, facts, path) {
  if (node.op === 'all' || node.op === 'any') {
    const children = node.args.map((child, index) => evaluate(child, facts, `${path}.${index}`));
    return { path, op: node.op, passed: node.op === 'all' ? children.every(child => child.passed) :
      children.some(child => child.passed), children };
  }
  if (node.op === 'not') {
    const child = evaluate(node.arg, facts, `${path}.0`);
    return { path, op: 'not', passed: !child.passed, children: [child] };
  }
  const actual = facts[node.fact];
  const expected = node.value;
  let passed = false;
  switch (node.op) {
    case 'eq': passed = actual === expected; break;
    case 'ne': passed = actual !== expected; break;
    case 'gt': passed = actual > expected; break;
    case 'gte': passed = actual >= expected; break;
    case 'lt': passed = actual < expected; break;
    case 'lte': passed = actual <= expected; break;
    case 'contains': passed = actual.includes(expected); break;
    case 'oneOf': passed = expected.includes(actual); break;
  }
  return { path, op: node.op, fact: node.fact, actual: structuredClone(actual), expected, passed };
}

export function explainExpression(expression, facts) {
  return evaluate(validateExpression(expression), facts, 'root');
}

export function evaluateRules(policy, facts) {
  const results = policy.rules.map(rule => {
    const applicability = rule.when === null ? null : explainExpression(rule.when, facts);
    const applicable = applicability === null || applicability.passed;
    const evidence = applicable ? explainExpression(rule.require, facts) : null;
    return { ruleId: rule.id, originVersionId: rule.originVersionId, severity: rule.severity,
      message: rule.message, status: !applicable ? 'skipped' : evidence.passed ? 'passed' : 'failed',
      applicability, evidence };
  });
  const blocking = results.filter(result => result.status === 'failed' && result.severity === 'error');
  return { passed: blocking.length === 0, blockingRuleIds: blocking.map(result => result.ruleId),
    warningCount: results.filter(result => result.status === 'failed' && result.severity === 'warning').length,
    results };
}
