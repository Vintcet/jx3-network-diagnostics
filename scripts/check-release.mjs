import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';

const read = path => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const app = JSON.parse(read('package.json'));
const config = JSON.parse(read('src-tauri/tauri.conf.json'));
const lock = JSON.parse(read('package-lock.json'));
const cargo = read('src-tauri/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1];
assert.match(app.version, /^\d+\.\d+\.\d+$/, '正式发布需要稳定版版本号');
for (const version of [config.version, lock.version, lock.packages[''].version, cargo]) {
  assert.equal(version, app.version, '前后端和锁文件版本号必须一致');
}
assert.match(app.releaseDate, /^\d{4}-\d{2}-\d{2}$/);
assert.equal(new Date(app.releaseDate).toISOString().slice(0, 10), app.releaseDate);
assert.ok(app.author.trim());
assert.ok(read('CHANGELOG.md').includes(`## ${app.version} — ${app.releaseDate}`), '缺少本版本更新记录');
if (process.env.GITHUB_REF_TYPE === 'tag') {
  assert.equal(process.env.GITHUB_REF_NAME, `v${app.version}`, 'Git 标签必须与应用版本一致');
}
assert.equal(config.bundle.createUpdaterArtifacts, true);
assert.deepEqual(config.plugins.updater.endpoints, ['https://github.com/Vintcet/jx3-network-diagnostics/releases/latest/download/latest.json']);
assert.ok(config.plugins.updater.pubkey);
console.log(`发布元数据一致：v${app.version} / ${app.releaseDate} / ${app.author}`);
