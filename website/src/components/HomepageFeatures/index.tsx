/**
 * @fileoverview Homepage features grid component.
 *
 * Displays a responsive grid of feature cards highlighting
 * Isolate's key capabilities. Used on the main landing page
 * to showcase what makes Isolate unique.
 *
 * @module components/HomepageFeatures
 */

import type {ReactNode} from 'react';
import clsx from 'clsx';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

/**
 * Configuration for a single feature card.
 * @typedef {Object} FeatureItem
 * @property {string} title - Feature title displayed as heading
 * @property {string} icon - Emoji icon for visual identification
 * @property {ReactNode} description - JSX description content
 */
type FeatureItem = {
  title: string;
  icon: string;
  description: ReactNode;
};

/**
 * List of features displayed on the homepage.
 * Each feature highlights a key capability of the Isolate runtime.
 *
 * Features include:
 * - Sub-5ms cold start performance
 * - Capability-based security model
 * - Resource limits and metering
 * - Multi-language WebAssembly support
 * - Production-ready observability
 * - Memory-safe Rust implementation
 *
 * @type {FeatureItem[]}
 */
const FeatureList: FeatureItem[] = [
  {
    title: 'Sub-5ms Cold Start',
    icon: '⚡',
    description: (
      <>
        Spin up isolated sandboxes in under 5 milliseconds. 25x faster than microVM-based
        solutions like Firecracker. Perfect for latency-sensitive workloads.
      </>
    ),
  },
  {
    title: 'Capability-Based Security',
    icon: '🔒',
    description: (
      <>
        Default-deny security model. Code has no access to filesystem, network, or
        environment unless explicitly granted. Every operation is audited.
      </>
    ),
  },
  {
    title: 'Resource Limits',
    icon: '📊',
    description: (
      <>
        Control CPU time, memory usage, and I/O quotas. Fuel-based instruction metering
        prevents infinite loops. Epoch-based timeout interruption.
      </>
    ),
  },
  {
    title: 'Multi-Language Support',
    icon: '🌐',
    description: (
      <>
        Run any language that compiles to WebAssembly: Rust, C/C++, Go, AssemblyScript,
        Python (via PyPy), and many more.
      </>
    ),
  },
  {
    title: 'Production Ready',
    icon: '🏭',
    description: (
      <>
        Built-in Prometheus metrics, OpenTelemetry tracing, and structured audit logs.
        gRPC server and CLI included for flexible deployment.
      </>
    ),
  },
  {
    title: 'Memory Safe',
    icon: '🛡️',
    description: (
      <>
        Written in Rust with minimal unsafe code. WASM's linear memory model provides
        strong isolation guarantees between sandboxes.
      </>
    ),
  },
];

/**
 * Individual feature card component.
 * Renders a single feature with icon, title, and description
 * in a styled card layout.
 *
 * @param {FeatureItem} props - Feature configuration
 * @param {string} props.title - Feature title
 * @param {string} props.icon - Emoji icon
 * @param {ReactNode} props.description - Description content
 * @returns {ReactNode} A styled feature card
 */
function Feature({title, icon, description}: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
      <div className={styles.featureCard}>
        <div className={styles.featureIcon}>{icon}</div>
        <div className={styles.featureContent}>
          <Heading as="h3" className={styles.featureTitle}>{title}</Heading>
          <p className={styles.featureDescription}>{description}</p>
        </div>
      </div>
    </div>
  );
}

/**
 * Homepage features section component.
 * Renders a responsive 3-column grid of feature cards highlighting
 * Isolate's key capabilities. Used on the main landing page.
 *
 * @returns {ReactNode} Features section with heading and card grid
 *
 * @example
 * // Usage in homepage
 * <HomepageFeatures />
 */
export default function HomepageFeatures(): ReactNode {
  return (
    <section className={styles.features}>
      <div className="container">
        <Heading as="h2" className="text--center" style={{marginBottom: '3rem'}}>
          Everything you need to run untrusted code
        </Heading>
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
