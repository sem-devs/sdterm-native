#!/usr/bin/env node
/**
 * Writes the update feed for one system next to the installer it points to.
 *
 *   node scripts/make-feed.mjs <win|linux|mac> <folder> <installer file name> [arch]
 *
 * The feed is `latest-<system>.json` in the same folder, in the shape the app reads:
 *   { "version", "notes", "releaseDate", "files": [ { "arch", "url", "sha512" (base64), "size" } ] }
 * `url` is the file name, relative to the feed. The version comes from Cargo.toml. Release notes
 * can be given in SDTERM_RELEASE_NOTES. The folder also gets SHA256SUMS-<system>.txt for every
 * installer in it.
 */
import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const [system, folder, installer, arch = 'x64'] = process.argv.slice(2)
if (!['win', 'linux', 'mac'].includes(system) || !folder || !installer) {
  console.error('Usage: node scripts/make-feed.mjs <win|linux|mac> <folder> <installer file name> [arch]')
  process.exit(1)
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const version = /^version = "(.*)"/m.exec(readFileSync(join(root, 'Cargo.toml'), 'utf8'))?.[1]
if (!version) throw new Error('no version in Cargo.toml')

const dir = resolve(folder)
const bytes = readFileSync(join(dir, installer))
const feed = {
  version,
  notes: process.env.SDTERM_RELEASE_NOTES ?? '',
  releaseDate: new Date().toISOString(),
  files: [
    {
      arch,
      url: installer,
      sha512: createHash('sha512').update(bytes).digest('base64'),
      size: statSync(join(dir, installer)).size
    }
  ]
}
writeFileSync(join(dir, `latest-${system}.json`), `${JSON.stringify(feed)}\n`)

const sums = readdirSync(dir)
  .filter((f) => /\.(zip|exe|deb|AppImage|tar\.gz|dmg)$/.test(f))
  .sort()
  .map((f) => `${createHash('sha256').update(readFileSync(join(dir, f))).digest('hex')}  ${f}`)
writeFileSync(join(dir, `SHA256SUMS-${system}.txt`), `${sums.join('\n')}\n`)
console.log(`latest-${system}.json for ${installer} (${feed.files[0].size} bytes), ${sums.length} checksums`)
