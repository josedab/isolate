import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    'intro',
    {
      type: 'category',
      label: 'Getting Started',
      collapsed: false,
      items: [
        'getting-started/installation',
        'getting-started/quick-start',
        'getting-started/first-sandbox',
      ],
    },
    {
      type: 'category',
      label: 'Guides',
      items: [
        'guides/capabilities',
        'guides/resource-limits',
        'guides/security-model',
        'guides/building-wasm-modules',
        'guides/use-cases',
        'guides/monitoring',
        'guides/cli',
        'guides/grpc-server',
        'guides/python-bindings',
        'guides/deployment',
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      items: [
        'reference/configuration',
        'reference/api',
        'reference/errors',
        'reference/benchmarks',
        'reference/troubleshooting',
        'reference/experimental',
      ],
    },
    {
      type: 'category',
      label: 'Internals',
      items: [
        'internals/architecture',
        'internals/wasm-engine',
        'internals/capability-system',
      ],
    },
    {
      type: 'category',
      label: 'Comparison',
      items: [
        'comparison/overview',
        'comparison/vs-wasmtime',
        'comparison/vs-microvms',
      ],
    },
    'contributing',
    'changelog',
  ],
};

export default sidebars;
