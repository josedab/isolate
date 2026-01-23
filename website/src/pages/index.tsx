import type {ReactNode} from 'react';
import {useState} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import Layout from '@theme/Layout';
import HomepageFeatures from '@site/src/components/HomepageFeatures';
import Heading from '@theme/Heading';
import CodeBlock from '@theme/CodeBlock';

import styles from './index.module.css';

const badges = [
  {
    alt: 'CI',
    src: 'https://github.com/josedab/isolate/workflows/CI/badge.svg',
    href: 'https://github.com/josedab/isolate/actions',
  },
  {
    alt: 'codecov',
    src: 'https://codecov.io/gh/josedab/isolate/branch/main/graph/badge.svg',
    href: 'https://codecov.io/gh/josedab/isolate',
  },
  {
    alt: 'Crates.io',
    src: 'https://img.shields.io/crates/v/isolate-core.svg',
    href: 'https://crates.io/crates/isolate-core',
  },
  {
    alt: 'License',
    src: 'https://img.shields.io/crates/l/isolate-core.svg',
    href: 'https://github.com/josedab/isolate/blob/main/LICENSE-MIT',
  },
];

const exampleCode = `use isolate_core::{Sandbox, SandboxConfig, capability::Capability};

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    let wasm_bytes = std::fs::read("plugin.wasm")?;

    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .memory_limit(128 * 1024 * 1024)  // 128MB
        .cpu_time_limit(std::time::Duration::from_secs(30))
        .capability(Capability::stdout())
        .capability(Capability::filesystem_read("/data"))
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit: {} | Output: {}", output.exit_code, output.stdout_str());
    Ok(())
}`;

function CopyButton({text}: {text: string}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <button
      className={clsx(styles.copyButton, copied && styles.copyButtonCopied)}
      onClick={handleCopy}
      title="Copy to clipboard"
    >
      {copied ? 'Copied!' : 'Copy'}
    </button>
  );
}

function HomepageHeader() {
  const {siteConfig} = useDocusaurusContext();
  return (
    <header className={clsx('hero', styles.heroBanner)}>
      <div className="container">
        <Heading as="h1" className={styles.heroTitle}>
          Run untrusted code safely.
          <br />
          <span className={styles.heroHighlight}>In milliseconds.</span>
        </Heading>
        <p className={styles.heroSubtitle}>
          Isolate is a secure sandbox runtime for executing WebAssembly code with
          capability-based security, resource limits, and sub-5ms cold starts.
        </p>

        <div className={styles.badges}>
          {badges.map((badge, idx) => (
            <a key={idx} href={badge.href} target="_blank" rel="noopener noreferrer">
              <img src={badge.src} alt={badge.alt} />
            </a>
          ))}
        </div>

        <div className={styles.installCommand}>
          <code>cargo add isolate-core</code>
          <CopyButton text="cargo add isolate-core" />
        </div>

        <div className={styles.buttons}>
          <Link
            className="button button--primary button--lg"
            to="/docs/getting-started/quick-start">
            Get Started
          </Link>
          <Link
            className="button button--secondary button--lg"
            to="https://github.com/josedab/isolate">
            GitHub
          </Link>
          <Link
            className="button button--secondary button--lg"
            to="https://docs.rs/isolate-core">
            API Docs
          </Link>
        </div>
      </div>
    </header>
  );
}

function MetricsSection() {
  return (
    <section className={styles.metrics}>
      <div className="container">
        <div className={styles.metricsGrid}>
          <div className={styles.metricCard}>
            <span className={styles.metricValue}>&lt;5ms</span>
            <span className={styles.metricLabel}>Cold Start (p99)</span>
          </div>
          <div className={styles.metricCard}>
            <span className={styles.metricValue}>&lt;500µs</span>
            <span className={styles.metricLabel}>Warm Start</span>
          </div>
          <div className={styles.metricCard}>
            <span className={styles.metricValue}>&lt;5MB</span>
            <span className={styles.metricLabel}>Memory Overhead</span>
          </div>
          <div className={styles.metricCard}>
            <span className={styles.metricValue}>80+</span>
            <span className={styles.metricLabel}>Tests Passing</span>
          </div>
        </div>
      </div>
    </section>
  );
}

function CodeExample() {
  return (
    <section className={styles.codeExample}>
      <div className="container">
        <div className={styles.codeExampleContent}>
          <div className={styles.codeExampleText}>
            <Heading as="h2">Secure by default</Heading>
            <p>
              Isolate uses a <strong>capability-based security model</strong> with default-deny.
              Code has no access to the filesystem, network, or environment unless explicitly granted.
            </p>
            <ul className={styles.featureList}>
              <li>Fine-grained filesystem permissions (read/write per path)</li>
              <li>Network access limited to specific hosts</li>
              <li>Environment variables explicitly allowlisted</li>
              <li>Resource limits enforced at runtime</li>
              <li>Full audit logging of all operations</li>
            </ul>
            <Link
              className="button button--primary"
              to="/docs/guides/capabilities">
              Learn about capabilities
            </Link>
          </div>
          <div className={styles.codeExampleCode}>
            <CodeBlock language="rust" title="main.rs">
              {exampleCode}
            </CodeBlock>
          </div>
        </div>
      </div>
    </section>
  );
}

function ComparisonSection() {
  return (
    <section className={styles.comparison}>
      <div className="container">
        <Heading as="h2" className="text--center">
          Why Isolate?
        </Heading>
        <p className="text--center" style={{maxWidth: '600px', margin: '0 auto 2rem'}}>
          Isolate combines the speed of WASM with built-in security controls that would
          take weeks to implement manually.
        </p>
        <div className={styles.comparisonTable}>
          <table>
            <thead>
              <tr>
                <th>Feature</th>
                <th>Isolate</th>
                <th>Bare Wasmtime</th>
                <th>microVMs</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Cold Start</td>
                <td className={styles.good}>&lt;5ms</td>
                <td className={styles.good}>&lt;5ms</td>
                <td className={styles.warning}>125ms+</td>
              </tr>
              <tr>
                <td>Memory Overhead</td>
                <td className={styles.good}>&lt;5MB</td>
                <td className={styles.good}>~2MB</td>
                <td className={styles.warning}>128MB+</td>
              </tr>
              <tr>
                <td>Capability System</td>
                <td className={styles.good}>Built-in</td>
                <td className={styles.warning}>Manual</td>
                <td className={styles.warning}>Varies</td>
              </tr>
              <tr>
                <td>Resource Metering</td>
                <td className={styles.good}>Built-in</td>
                <td className={styles.warning}>Manual</td>
                <td className={styles.neutral}>OS-level</td>
              </tr>
              <tr>
                <td>Multi-tenant Ready</td>
                <td className={styles.good}>Yes</td>
                <td className={styles.warning}>Manual</td>
                <td className={styles.good}>Yes</td>
              </tr>
              <tr>
                <td>Audit Logging</td>
                <td className={styles.good}>Built-in</td>
                <td className={styles.warning}>Manual</td>
                <td className={styles.warning}>Manual</td>
              </tr>
            </tbody>
          </table>
        </div>
        <div className="text--center" style={{marginTop: '2rem'}}>
          <Link
            className="button button--secondary"
            to="/docs/comparison/overview">
            See full comparison
          </Link>
        </div>
      </div>
    </section>
  );
}

function UseCasesSection() {
  const useCases = [
    {
      title: 'Plugin Systems',
      description: 'Run third-party plugins without risking your application. Each plugin runs in complete isolation.',
      icon: '🔌',
    },
    {
      title: 'Serverless Functions',
      description: 'Execute user-provided code in multi-tenant environments with guaranteed isolation between tenants.',
      icon: '☁️',
    },
    {
      title: 'Code Sandboxing',
      description: 'Safely run untrusted code snippets for testing, education, or CI/CD pipelines.',
      icon: '📦',
    },
    {
      title: 'Edge Computing',
      description: 'Deploy lightweight, isolated workloads close to users with minimal cold start latency.',
      icon: '🌐',
    },
  ];

  return (
    <section className={styles.useCases}>
      <div className="container">
        <Heading as="h2" className="text--center">
          Built for Production
        </Heading>
        <p className="text--center" style={{maxWidth: '600px', margin: '0 auto 2rem'}}>
          Isolate is designed for security-critical workloads where you need to run untrusted code.
        </p>
        <div className={styles.useCasesGrid}>
          {useCases.map((useCase, idx) => (
            <div key={idx} className={styles.useCaseCard}>
              <span className={styles.useCaseIcon}>{useCase.icon}</span>
              <Heading as="h3">{useCase.title}</Heading>
              <p>{useCase.description}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function BuiltOnSection() {
  return (
    <section className={styles.builtOn}>
      <div className="container">
        <div className={styles.builtOnContent}>
          <div className={styles.builtOnText}>
            <span className={styles.builtOnLabel}>Powered by</span>
            <Heading as="h3">
              <a href="https://wasmtime.dev/" target="_blank" rel="noopener noreferrer">
                Wasmtime
              </a>
            </Heading>
            <p>
              Built on the{' '}
              <a href="https://bytecodealliance.org/" target="_blank" rel="noopener noreferrer">
                Bytecode Alliance's
              </a>{' '}
              industry-leading WebAssembly runtime. Wasmtime is used in production by
              Fastly, Shopify, and other major platforms.
            </p>
          </div>
          <div className={styles.builtOnLogos}>
            <div className={styles.logoItem}>
              <span className={styles.logoIcon}>🦀</span>
              <span>100% Rust</span>
            </div>
            <div className={styles.logoItem}>
              <span className={styles.logoIcon}>🔐</span>
              <span>Memory Safe</span>
            </div>
            <div className={styles.logoItem}>
              <span className={styles.logoIcon}>⚡</span>
              <span>Native Speed</span>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function CTASection() {
  return (
    <section className={styles.cta}>
      <div className="container">
        <Heading as="h2">Ready to get started?</Heading>
        <p>
          Add Isolate to your project and run your first sandboxed WASM module in minutes.
        </p>
        <div className={styles.buttons}>
          <Link
            className="button button--primary button--lg"
            to="/docs/getting-started/quick-start">
            Quick Start Guide
          </Link>
          <Link
            className="button button--secondary button--lg"
            to="/docs/">
            Read the Docs
          </Link>
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Secure Sandbox Runtime for WebAssembly"
      description="Isolate is a lightweight, secure sandbox runtime for executing untrusted WebAssembly code with capability-based security, resource limits, and sub-5ms cold starts.">
      <HomepageHeader />
      <main>
        <MetricsSection />
        <HomepageFeatures />
        <CodeExample />
        <ComparisonSection />
        <UseCasesSection />
        <BuiltOnSection />
        <CTASection />
      </main>
    </Layout>
  );
}
