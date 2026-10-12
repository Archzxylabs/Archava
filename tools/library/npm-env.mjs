/* global process */
/**
 * Environment for the cold-consumer `npm install` that every proof and runtime
 * build runs against packed tarballs.
 *
 * pnpm re-exports the invoking user's npm configuration into each script it
 * runs, so a user-level `allow-scripts` policy reaches the child as
 * `npm_config_allow_scripts`. npm reads environment configuration as a
 * CLI-level policy and refuses it outright for project-scoped installs
 * (`EALLOWSCRIPTS`), which fails the consumer before a single package is
 * resolved. Every call site already installs with `--ignore-scripts`, so no
 * lifecycle script runs either way: the inherited policy is dropped for this
 * child instead of being rewritten in the owner's `~/.npmrc`.
 *
 * The returned object is a shallow copy; nothing else about the caller's
 * environment changes.
 *
 * @param {Record<string, string>} [base]
 * @returns {Record<string, string>}
 */
export const installEnv = (base = process.env) => {
  const env = { ...base }
  delete env.npm_config_allow_scripts
  return env
}
