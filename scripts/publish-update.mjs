#!/usr/bin/env node
/**
 * Publishes a built version so installed apps can find and install it.
 *
 *   cd scripts && npm install        (once)
 *   BLOB_READ_WRITE_TOKEN=... node scripts/publish-update.mjs dist/<version>
 *
 * The folder may hold the macOS files from scripts/package-mac.sh and, in it or in sub-folders,
 * the Windows and Linux files of the "Build" workflow (`gh run download <run> -D dist/<version>`
 * puts each artifact in a folder of its own, which is fine). It uploads every installer, zip and
 * checksum file, then the feeds: latest-mac.json, latest-win.json and latest-linux.json. Before
 * uploading, each feed is checked against the files it names (they must exist and match the
 * SHA-512 in the feed). The feeds go last, so an app never reads one that points to a file that is
 * not there yet, and they are cached for one minute only so a release shows up quickly.
 */
import { createHash } from 'node:crypto'
import { createReadStream, existsSync, readdirSync, readFileSync } from 'node:fs'
import { basename, join, resolve } from 'node:path'
import { put } from '@vercel/blob'

const token = process.env.BLOB_READ_WRITE_TOKEN
const dir = resolve(process.argv[2] ?? '')
if (!token || !process.argv[2] || !existsSync(dir)) {
  console.error('Usage: BLOB_READ_WRITE_TOKEN=... node scripts/publish-update.mjs dist/<version>')
  process.exit(1)
}

const PREFIX = 'sdterm/native-updates'

// Every file in the folder and below it, by name.
const found = new Map()
for (const rel of readdirSync(dir, { recursive: true })) {
  const name = basename(rel)
  const full = join(dir, rel)
  if (found.has(name) || !existsSync(full)) continue
  found.set(name, full)
}

const feeds = [...found.keys()].filter((f) => /^latest-.*\.json$/.test(f))
if (feeds.length === 0) {
  console.error(`No latest-*.json in ${dir}. Build first.`)
  process.exit(1)
}

// Every installer a feed names, checked, plus the other downloads and the checksums next to them.
const files = new Set()
for (const feed of feeds) {
  const { version, files: named } = JSON.parse(readFileSync(found.get(feed), 'utf8'))
  for (const f of named) {
    const path = found.get(f.url)
    if (!path) throw new Error(`${feed} names ${f.url}, which is not in ${dir}`)
    const sha = createHash('sha512').update(readFileSync(path)).digest('base64')
    if (sha !== f.sha512) throw new Error(`${feed}: ${f.url} does not match its sha512, build it again`)
    files.add(f.url)
  }
  console.log(`${feed}: version ${version}, ${named.length} file(s) checked`)
}
for (const name of found.keys()) {
  if (/\.(dmg|exe|msi|deb|zip|tar\.gz|AppImage)$/.test(name) || name.startsWith('SHA256SUMS')) files.add(name)
}

const upload = async (file, cacheControlMaxAge) => {
  const path = found.get(file)
  if (!path) throw new Error(`Missing ${file}`)
  const blob = await put(`${PREFIX}/${file}`, createReadStream(path), {
    access: 'public',
    token,
    addRandomSuffix: false,
    allowOverwrite: true,
    multipart: true,
    contentType: file.endsWith('.json') ? 'application/json; charset=utf-8' : 'application/octet-stream',
    ...(cacheControlMaxAge ? { cacheControlMaxAge } : {})
  })
  console.log(`  ${blob.url}`)
}

for (const file of files) {
  console.log(`Uploading ${file} ...`)
  await upload(file)
}
for (const feed of feeds) {
  console.log(`Publishing ${feed} ...`)
  await upload(feed, 60)
}
console.log('Published. Installed apps will see it within a few hours, or on Check now.')
