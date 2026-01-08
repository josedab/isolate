import type {ReactNode} from 'react';
import clsx from 'clsx';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  icon: string;
  description: ReactNode;
};

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
