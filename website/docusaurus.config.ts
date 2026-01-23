import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Isolate',
  tagline: 'Run untrusted code safely. In milliseconds.',
  favicon: 'img/favicon.ico',

  future: {
    v4: true,
  },

  url: 'https://josedab.github.io',
  baseUrl: '/isolate/',

  organizationName: 'josedab',
  projectName: 'isolate',

  onBrokenLinks: 'throw',
  onBrokenMarkdownLinks: 'warn',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  markdown: {
    mermaid: true,
  },

  themes: [
    '@docusaurus/theme-mermaid',
    [
      '@easyops-cn/docusaurus-search-local',
      {
        hashed: true,
        language: ['en'],
        highlightSearchTermsOnTargetPage: true,
        explicitSearchResultPath: true,
        docsRouteBasePath: '/docs',
        blogRouteBasePath: '/blog',
        indexBlog: true,
        indexDocs: true,
        indexPages: false,
      },
    ],
  ],

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/josedab/isolate/tree/main/website/',
          showLastUpdateTime: true,
          showLastUpdateAuthor: true,
        },
        blog: {
          showReadingTime: true,
          feedOptions: {
            type: ['rss', 'atom'],
            xslt: true,
          },
          editUrl: 'https://github.com/josedab/isolate/tree/main/website/',
          onInlineTags: 'warn',
          onInlineAuthors: 'warn',
          onUntruncatedBlogPosts: 'warn',
        },
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/isolate-social-card.png',
    metadata: [
      {name: 'keywords', content: 'wasm, webassembly, sandbox, rust, security, isolation'},
      {name: 'twitter:card', content: 'summary_large_image'},
    ],
    colorMode: {
      defaultMode: 'dark',
      disableSwitch: false,
      respectPrefersColorScheme: true,
    },
    announcementBar: {
      id: 'announcement',
      content: 'Isolate is currently in early development (v0.1.x). APIs may change.',
      backgroundColor: '#1a1a2e',
      textColor: '#e94560',
      isCloseable: true,
    },
    navbar: {
      title: 'Isolate',
      logo: {
        alt: 'Isolate Logo',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Docs',
        },
        {
          to: '/docs/getting-started/quick-start',
          label: 'Quick Start',
          position: 'left',
        },
        {
          to: '/docs/reference/api',
          label: 'API',
          position: 'left',
        },
        {to: '/blog', label: 'Blog', position: 'left'},
        {
          href: 'https://docs.rs/isolate-core',
          label: 'docs.rs',
          position: 'right',
        },
        {
          href: 'https://crates.io/crates/isolate-core',
          label: 'crates.io',
          position: 'right',
        },
        {
          href: 'https://github.com/josedab/isolate',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Learn',
          items: [
            {
              label: 'Quick Start',
              to: '/docs/getting-started/quick-start',
            },
            {
              label: 'Capabilities',
              to: '/docs/guides/capabilities',
            },
            {
              label: 'Security Model',
              to: '/docs/guides/security-model',
            },
          ],
        },
        {
          title: 'Reference',
          items: [
            {
              label: 'API Reference',
              to: '/docs/reference/api',
            },
            {
              label: 'Configuration',
              to: '/docs/reference/configuration',
            },
            {
              label: 'docs.rs',
              href: 'https://docs.rs/isolate-core',
            },
          ],
        },
        {
          title: 'Community',
          items: [
            {
              label: 'GitHub',
              href: 'https://github.com/josedab/isolate',
            },
            {
              label: 'Issues',
              href: 'https://github.com/josedab/isolate/issues',
            },
            {
              label: 'Discussions',
              href: 'https://github.com/josedab/isolate/discussions',
            },
          ],
        },
        {
          title: 'More',
          items: [
            {
              label: 'Blog',
              to: '/blog',
            },
            {
              label: 'Changelog',
              to: '/docs/changelog',
            },
            {
              label: 'Contributing',
              to: '/docs/contributing',
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Isolate Contributors. MIT OR Apache-2.0 Licensed.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['rust', 'toml', 'bash', 'protobuf'],
    },
    // Local search is configured via @easyops-cn/docusaurus-search-local theme
  } satisfies Preset.ThemeConfig,
};

export default config;
