// `autopilot observe` — file one observation into the project's ledger, then exit.
//
// Filing is a deliberate, reviewable ledger write — kept separate from running the
// agent (`autopilot run`), which only reads. A human files with this; a monitoring
// adapter would drop the same shape directly. See docs/adr/0006.
//
//   autopilot observe "support reports bulk user export is missing"
//   autopilot observe --source cloudwatch "error rate on /users spiked to 12%"
//
// The project is where the command is run (docs/adr/0007).

import { appendObservation } from './observations.js'
import { currentProject } from './project.js'

export async function observe(argv: string[]): Promise<void> {
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
    console.error('usage: autopilot observe [--source <name>] "what was noticed or wanted"')
    process.exit(2)
  }

  const project = await currentProject()
  const id = await appendObservation(project.observationsDir, { source, body })
  console.error(`filed observation ${id} (source: ${source}) in ${project.observationsDir}`)
}
