import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { packVersion, incrementVersion, synchronizeVersion, versionFiles } from './version.mjs';

const git = (root,args,env) => execFileSync('git',args,{ cwd:root,env,encoding:'utf8',stdio:['ignore','pipe','pipe'] }).trim();
function revision(value) { if (!/^[a-f0-9]{40}$/.test(value ?? '')) throw new Error('expected full source SHA'); return value; }
export function releaseGuard(root, expected, remote = 'origin', env) {
  revision(expected);
  if (git(root,['branch','--show-current']) !== 'main') throw new Error('release requires main');
  if (git(root,['status','--porcelain'])) throw new Error('release requires a clean checkout');
  if (git(root,['ls-remote',remote,'refs/heads/main'],env).split(/\s/)[0] !== expected) throw new Error('main advanced; release blocked');
}
export function prepareRelease(root, expected, bump = 'current', remote = 'origin', env) {
  releaseGuard(root,expected,remote,env);
  if (git(root,['rev-parse','HEAD']) !== expected) throw new Error('checkout differs from selected revision');
  synchronizeVersion(root,undefined,true);
  const version = bump === 'current' ? packVersion(root) : incrementVersion(packVersion(root),bump);
  if (git(root,['ls-remote',remote,`refs/tags/v${version}`,`refs/tags/v${version}^{}`],env)) throw new Error('release tag already exists');
  if (git(root,['tag','--list',`v${version}`])) throw new Error('local release tag already exists');
  synchronizeVersion(root,version);
  const changed = git(root,['diff','--name-only']).split('\n').filter(Boolean);
  if (changed.some(file => !versionFiles(root).includes(file))) throw new Error('unexpected release changes');
  if (changed.length) {
    git(root,['add','--',...changed]);
    git(root,['-c','user.name=github-actions[bot]','-c','user.email=41898282+github-actions[bot]@users.noreply.github.com','commit','-m',`Release v${version}`]);
  }
  return { version, source: expected, revision: git(root,['rev-parse','HEAD']) };
}
export function publishReleaseRefs(root, expected, version, validatedRevision, remote = 'origin', env) {
  releaseGuard(root,expected,remote,env);
  synchronizeVersion(root,version,true);
  if (git(root,['rev-parse','HEAD']) !== revision(validatedRevision)) throw new Error('validated revision changed');
  const marker = JSON.parse(fs.readFileSync(path.join(root,'dist/distribution/.engineering-distribution.json'),'utf8'));
  if (marker.version !== version) throw new Error('distribution version mismatch');
  const commits = git(root,['rev-list',`${expected}..HEAD`]).split('\n').filter(Boolean);
  if (commits.length > 1) throw new Error('unexpected release history');
  if (commits.length && git(root,['diff','--name-only',expected,'HEAD']).split('\n').some(file => !versionFiles(root).includes(file))) throw new Error('release commit changed unexpected files');
  if (git(root,['ls-remote',remote,`refs/tags/v${version}`,`refs/tags/v${version}^{}`],env)) throw new Error('release tag already exists');
  git(root,['tag',`v${version}`,validatedRevision]);
  // A concurrent main/tag update causes the entire push to fail; never force either ref.
  git(root,['push','--atomic',remote,'HEAD:refs/heads/main',`refs/tags/v${version}`],env);
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('../',import.meta.url));
  if (process.env.GITHUB_ACTIONS !== 'true' || process.env.GITHUB_REF !== 'refs/heads/main' || process.env.GITHUB_EVENT_NAME !== 'workflow_dispatch' || process.env.GITHUB_REPOSITORY !== 'Aliaksie/engineering-pack') throw new Error('release requires an authorized manual main workflow');
  const token = process.env.GH_TOKEN;
  if (!token) throw new Error('release token unavailable');
  const env = { ...process.env, GIT_CONFIG_COUNT:'1', GIT_CONFIG_KEY_0:'http.https://github.com/.extraheader', GIT_CONFIG_VALUE_0:`AUTHORIZATION: basic ${Buffer.from(`x-access-token:${token}`).toString('base64')}` };
  delete env.GH_TOKEN;
  try {
    if (process.argv[2] === 'prepare') {
      const result = prepareRelease(root,process.env.GITHUB_SHA,process.env.RELEASE_BUMP,'origin',env);
      fs.appendFileSync(process.env.GITHUB_OUTPUT,`version=${result.version}\nrevision=${result.revision}\n`);
    } else if (process.argv[2] === 'publish') publishReleaseRefs(root,process.env.GITHUB_SHA,process.env.RELEASE_VERSION,process.env.RELEASE_REVISION,'origin',env);
    else throw new Error('expected prepare or publish');
  } catch { process.stderr.write('Release preparation/publication failed; check revision, version, tag and repository permissions.\n'); process.exitCode = 1; }
}
