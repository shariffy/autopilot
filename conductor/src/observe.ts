// File one observation into the ledger, then exit.
//
// Filing is a deliberate, reviewable ledger write — kept separate from running the
// agent (`npm start`), which only reads. A human files with this; a monitoring
// adapter would drop the same shape directly. See docs/adr/0006.
//
//   npm run observe -- "support reports bulk user export is missing"
//   npm run observe -- --source cloudwatch "error rate on /users spiked to 12%"
//
// Env: OBSERVATIONS overrides the ledger directory (default: ./observations).

import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { appendObservation } from './observations.js'

const here = path.dirname(fileURLToPath(import.meta.url))

async function main() {
  const argv = process.argv.slice(2)
  let source = 'human'
  const rest: string[] = []
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--source') {
      source = argv[++i] ?? 'human'
      continue
    }
    rest.push(argv[i])
  }

  const body = rest.join(' ').trim()
  if (!body) {
    console.error('usage: npm run observe -- [--source <name>] "what was noticed or wanted"')
    process.exit(2)
  }

  const dir = process.env.OBSERVATIONS
    ? path.resolve(process.env.OBSERVATIONS)
    : path.join(here, '..', 'observations')

  const id = await appendObservation(dir, { source, body })
  console.error(`filed observation ${id} (source: ${source}) in ${dir}`)
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
