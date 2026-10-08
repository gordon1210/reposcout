import { readdir } from 'node:fs/promises';

const directory = new URL('./', import.meta.url);
const names = (await readdir(directory)).filter(name => name.endsWith('.test.mjs')).sort();
let failed = 0;
for (const name of names) {
  try {
    const check = await import(new URL(name, directory));
    await check.default();
    process.stdout.write(`PASS ${name}\n`);
  } catch (error) {
    failed += 1;
    process.stderr.write(`FAIL ${name}\n${error.stack}\n`);
  }
}
process.stdout.write(`${names.length - failed}/${names.length} checks passed\n`);
process.exitCode = failed > 0 ? 1 : 0;
