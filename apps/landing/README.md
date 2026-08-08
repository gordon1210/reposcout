# RepoScout landing page

The public-facing RepoScout product site. It uses the same React, TypeScript, Vite,
and Tailwind CSS foundation as the dashboard, with a bespoke component and visual
system instead of Shadcn. Exact dependency and tool versions live in the package
manifests and root lockfile rather than being duplicated here.

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
