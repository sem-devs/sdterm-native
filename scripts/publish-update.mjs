#!/usr/bin/env node
/**
 * Publishes a built version so installed apps can find and install it.
 *
 *   cd scripts && npm install        (once)
 *   BLOB_READ_WRITE_TOKEN=... node scripts/publish-update.mjs dist/<version>
 *
 * Uploads the zips and DMGs of that folder, then the latest-*.json feed files. The feeds go last,
 * so an app never reads one that points to a file that is not there yet, and they are cached for
 * one minute only so a release shows up quickly.
 */
import { createReadStream, existsSync, readdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { put } from '@vercel/blob'

const token = process.env.BLOB_READ_WRITE_TOKEN
const dir = resolve(process.argv[2] ?? '')
if (!token || !process.argv[2] || !existsSync(dir)) {
  console.error('Usage: BLOB_READ_WRITE_TOKEN=... node scripts/publish-update.mjs dist/<version>')
  process.exit(1)
}

const PREFIX = 'sdterm/native-updates'
const feeds = readdirSync(dir).filter((f) => /^latest-.*\.json$/.test(f))
if (feeds.length === 0) {
  console.error(`No latest-*.json in ${dir}. Build first.`)
  process.exit(1)
}

// Every installer a feed names, plus the DMGs and the checksums that sit next to them.
const files = new Set()
for (const feed of feeds) {
  for (const f of JSON.parse(readFileSync(join(dir, feed), 'utf8')).files) files.add(f.url)
}
for (const f of readdirSync(dir)) if (/\.(dmg|exe|msi|deb|tar\.gz|AppImage)$/.test(f) || f.startsWith('SHA256SUMS')) files.add(f)

const upload = async (file, cacheControlMaxAge) => {
  const path = join(dir, file)
  if (!existsSync(path)) throw new Error(`Missing ${file}`)
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
