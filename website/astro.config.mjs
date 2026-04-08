import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { ion } from 'starlight-ion-theme';

export default defineConfig({
  site: 'https://oxicast.kribl.io',
  integrations: [
    starlight({
      title: '\u{1F6F0}\u{FE0F} oxicast',
      description:
        'Async Google Cast (Chromecast) client for Rust, built on tokio',
      plugins: [
        ion({
          icons: { iconDir: './src/icons' },
          footer: {
            text: '\u{1F6F0}\u{FE0F} oxicast',
            links: [
              {
                text: 'GitHub',
                href: 'https://github.com/denniskribl/oxicast',
              },
              {
                text: 'crates.io',
                href: 'https://crates.io/crates/oxicast',
              },
            ],
          },
        }),
      ],
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/denniskribl/oxicast',
        },
      ],
      editLink: {
        baseUrl: 'https://github.com/denniskribl/oxicast/edit/main/',
      },
      customCss: ['./src/styles/custom.css'],
      sidebar: [
        { label: 'Introduction', slug: 'index' },
        { label: 'README', slug: 'readme' },
        {
          label: 'Guides',
          items: [
            { label: 'Getting Started', slug: 'getting-started' },
            { label: 'API Overview', slug: 'api-overview' },
            { label: 'Error Handling', slug: 'error-handling' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'Architecture', slug: 'architecture' },
          ],
        },
      ],
    }),
  ],
});
