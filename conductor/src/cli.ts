// The `charter` command. The system is operated from inside a project directory,
// like git: `charter init` makes the current directory a project, and every other
// command resolves the project from where it is run — the nearest ancestor holding
// project.json. See docs/adr/0007.

import { init } from './project.js'
import { observe } from './observe.js'
import { run } from './run.js'

const USAGE = `usage: charter <command>

  charter init                                  make the current directory a project
  charter observe [--source <name>] "…need…"    file an observation into the ledger
  charter run [--dry-run]                       act on the observation ledger

commands run against the project you are standing in (the nearest ancestor
directory holding project.json).`

async function main() {
  const [cmd, ...rest] = process.argv.slice(2)
  switch (cmd) {
    case 'init':
      return init()
    case 'observe':
      return observe(rest)
    case 'run':
      return run(rest)
    case undefined:
    case 'help':
    case '--help':
      console.error(USAGE)
      process.exit(cmd ? 0 : 2)
      break
    default:
      console.error(`charter: unknown command "${cmd}"\n\n${USAGE}`)
      process.exit(2)
  }
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : e)
  process.exit(1)
})
