/* global process */
/**
 * The P-MODULE-CONSUMERS witness map, exported separately from
 * `module-consumers.ts` so `node --test` can assert it against the frozen
 * manifest without launching tsx, the pack, or npm.
 *
 * Witnesses here are [consumer, stage] pairs, not [suite, test-name] pairs as
 * in the cargo-backed producers: the two stages are the pieces of the driver
 * a consumer run must satisfy — its verify battery exiting clean with the
 * installed resolution `verify` prints PASS for, and the resolve()-inside-
 * node_modules plus implementation-hash equality that makes the pair
 * independent. The stage names are the same strings the proof JSON cites, so
 * the drift test can assert the map against the manifest while the run
 * records them as details.
 */

/** The accepted R06 modules this proof covers. */
export const MODULES = [
  '@archava/purchasing',
  '@archava/purchasing-pg',
  '@archava/foodservice',
  '@archava/foodservice-pg',
]

/** The two product verticals installed as consumers. */
export const CONSUMERS = ['hospitality', 'professional-services']

/**
 * Manifest required check → the consumer stages that produce it.
 *
 * Both checks are graded PER CONSUMER: "two consumers" is witnessed by the
 * two batteries that each consume BOTH modules, and the no-product-path rule
 * is witnessed by the two resolve() assertions that answer inside
 * node_modules only.
 */
export const WITNESSES = {
  'every accepted module has two independently installed consumers': [
    ['hospitality', 'installed-verify-battery-passes-for-both-modules'],
    ['professional-services', 'installed-verify-battery-passes-for-both-modules'],
  ],
  'no workspace/path dependency in either consumer': [
    ['hospitality', 'resolve-answers-in-node_modules-and-hashes-match'],
    ['professional-services', 'resolve-answers-in-node_modules-and-hashes-match'],
  ],
}
