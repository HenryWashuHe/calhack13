import { readFile } from 'node:fs/promises';
import { Actor } from 'apify';
import { extract } from './extractor.js';
import { createSourcePolicy, executeAndPublish } from './runtime.js';

// This file is baked into the source package. Actor input cannot amend policy.
const policy = createSourcePolicy(JSON.parse(await readFile(new URL('../allowed-sources.json', import.meta.url), 'utf8')));
await Actor.main(async () => {
  const input = await Actor.getInput<unknown>();
  await executeAndPublish(input, extract, policy, async (outcome) => {
    await Actor.pushData(outcome);
  });
});
