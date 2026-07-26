# RepoScout landing page

The public-facing RepoScout product site. It uses the same React 19, TypeScript 6,
Vite 8, and Tailwind CSS 4 foundation as the dashboard, with a bespoke component
and visual system instead of Shadcn.

The hero imports the shared high-resolution RepoScout artwork from
`apps/web/src/assets/reposcout.png` so the source asset remains canonical.
The navigation, footer, final call to action, and favicon use the transparent
`src/assets/reposcout-fox.png` brand icon.

```sh
pnpm install
pnpm dev:landing
pnpm build:landing
```

The production output is written to `apps/landing/dist`.
