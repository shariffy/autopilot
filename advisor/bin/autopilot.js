#!/usr/bin/env node
// The `autopilot` bin: tsx runs the TypeScript sources directly, so there is no
// build step. `npm link` in advisor/ puts this on PATH.
import { register } from 'tsx/esm/api'

register()
await import(new URL('../src/cli.ts', import.meta.url).href)
