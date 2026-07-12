#!/usr/bin/env node
// The `charter` bin: tsx runs the TypeScript sources directly, so there is no
// build step. `npm link` in conductor/ puts this on PATH.
import { register } from 'tsx/esm/api'

register()
await import(new URL('../src/cli.ts', import.meta.url).href)
