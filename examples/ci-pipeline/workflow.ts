/**
 * Example CI Pipeline Workflow
 *
 * This demonstrates a typical CI/CD pipeline defined as a Petri-net workflow
 * using the Circuit Breaker SDK.
 *
 * The pipeline:
 * 1. Starts with source code ready
 * 2. Builds the project
 * 3. Runs tests, linting, and security scans in parallel (fan-out)
 * 4. Merges results (fan-in / AND-join)
 * 5. Deploys if on main branch (guarded transition)
 */

import { workflow } from '@circuit-breaker/core';

const ciPipeline = workflow('ci-pipeline')
  .namespace('examples')
  .description('Example CI/CD pipeline demonstrating Petri-net workflow patterns')
  .labels({
    team: 'platform',
    type: 'ci',
    language: 'typescript',
  })

  // ============ Places (States) ============

  // Initial state - source code is ready
  .place('source-ready', { initialTokens: 1 })

  // After build
  .place('build-complete')

  // Parallel check results
  .place('tests-passed')
  .place('lint-passed')
  .place('security-passed')

  // Synchronization point after all checks
  .place('all-checks-passed')

  // Final states
  .place('deployed')
  .place('artifacts-stored')

  // ============ Transitions (Actions) ============

  // Build transition
  .transition('build')
    .from('source-ready')
    .to('build-complete')
    .dagger('./ci', 'build', {
      target: 'release',
      platform: 'linux/amd64',
    })
    .resources({ cpu: '2', memory: '4Gi' })
    .timeout('15m')
    .retries(2)
    .done()

  // Parallel fan-out: build-complete triggers three parallel paths

  // Test transition
  .transition('test')
    .from('build-complete')
    .to('tests-passed')
    .dagger('./ci', 'test', {
      coverage: true,
      parallel: 4,
    })
    .resources({ cpu: '4', memory: '8Gi' })
    .timeout('20m')
    .retries(1)
    .done()

  // Lint transition
  .transition('lint')
    .from('build-complete')
    .to('lint-passed')
    .dagger('./ci', 'lint')
    .resources({ cpu: '1', memory: '2Gi' })
    .timeout('5m')
    .done()

  // Security scan transition
  .transition('security-scan')
    .from('build-complete')
    .to('security-passed')
    .dagger('./ci', 'security', {
      scanners: ['trivy', 'grype'],
      failOnHigh: true,
    })
    .resources({ cpu: '2', memory: '4Gi' })
    .timeout('10m')
    .done()

  // Fan-in (AND-join): requires all three checks to pass
  .transition('merge-checks')
    .from('tests-passed', 'lint-passed', 'security-passed')
    .to('all-checks-passed')
    .noop() // Synchronization point only, no action needed
    .done()

  // Conditional deploy - only on main branch
  .transition('deploy')
    .from('all-checks-passed')
    .to('deployed')
    .guard('ctx.ref == "refs/heads/main" && ctx.event != "pull_request"')
    .dagger('./ci', 'deploy', {
      environment: 'production',
      strategy: 'rolling',
    })
    .resources({ cpu: '1', memory: '2Gi' })
    .timeout('10m')
    .retries(3, 'exponential')
    .priority(80) // Higher priority than other transitions
    .done()

  // Store artifacts (runs in parallel with deploy on main, or alone on PRs)
  .transition('store-artifacts')
    .from('all-checks-passed')
    .to('artifacts-stored')
    .dagger('./ci', 'upload-artifacts', {
      paths: ['dist/**', 'coverage/**'],
      retention: '30d',
    })
    .timeout('5m')
    .done()

  .build();

// Export the workflow
export default ciPipeline;

// Also export for named imports
export { ciPipeline };

/**
 * Alternative: You can also define workflows programmatically
 * for more dynamic use cases:
 */
export function createPipeline(options: {
  name: string;
  deployEnvironment: string;
  enableSecurityScan: boolean;
}) {
  let builder = workflow(options.name)
    .place('start', { initialTokens: 1 })
    .place('built')
    .place('tested')
    .place('ready')
    .place('done')

    .transition('build')
      .from('start')
      .to('built')
      .dagger('./ci', 'build')
      .done()

    .transition('test')
      .from('built')
      .to('tested')
      .dagger('./ci', 'test')
      .done();

  // Conditionally add security scan
  if (options.enableSecurityScan) {
    builder = builder
      .place('scanned')
      .transition('security')
        .from('tested')
        .to('scanned')
        .dagger('./ci', 'security')
        .done()
      .transition('merge')
        .from('scanned')
        .to('ready')
        .noop()
        .done();
  } else {
    builder = builder
      .transition('skip-security')
        .from('tested')
        .to('ready')
        .noop()
        .done();
  }

  return builder
    .transition('deploy')
      .from('ready')
      .to('done')
      .dagger('./ci', 'deploy', { environment: options.deployEnvironment })
      .done()
    .build();
}
