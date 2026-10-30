# Metric Owl public website

Live at **https://metricowl.cloud/**. English product site; the private monitoring
panel is not exposed through this hostname. Public contact: services@metricowl.cloud.

## Source and content

- `public/index.html`, `styles.css`, `site.js`: responsive product page and accessible
  screenshot gallery, keyboard tabs, image dialog, mobile navigation and native FAQs.
- `scripts/build-docs.mjs`: setup guide, development notes, privacy, 404, sitemap.
- `public/assets/`: locally served fonts, font license, vector brand mark and four
  actual application screenshots rendered with explicitly synthetic data.
- `scripts/capture-app.mjs`: screenshot fixture that renders the existing compiled
  React application. It never authenticates to an owner installation. No real
  hostname, private path, provider key or user credential belongs in public assets.
- `deploy/`: isolated Nginx vhost, certificate hook, static publisher and rollback notes.

This site does not publish an application binary, source repository, payment offer,
public MCP endpoint or hosted notification enrollment. Preview and roadmap claims
are deliberate. Update them only after the corresponding release is verified.

## Local work

Run from the repository root, with the existing web development dependencies installed:

```sh
node website/scripts/build-docs.mjs
python3 -m http.server 4886 --bind 127.0.0.1 --directory website/public
```

The following are optional asset regeneration steps (keep the server running):

```sh
# Requires the application's existing web/dist build.
node website/scripts/capture-app.mjs
node website/scripts/social-preview.mjs
```

Verify the static site locally or after publication:

```sh
node website/scripts/verify.mjs
WEBSITE_URL=https://metricowl.cloud node website/scripts/verify.mjs
```

Verification covers five widths (320, 390, 768, 1024, 1440), image tabs/dialog,
keyboard controls, mobile navigation, FAQs, internal links, docs, browser errors,
and absence of third-party browser requests. Screenshots and evidence are saved
under the ignored `artifacts/` directory.

Only `website/public` is published. The server uses versioned static releases and
an atomic `current` symlink. See `deploy/README.md` for activation and rollback.
The source code, scripts, app data and SMTP credentials are not web assets.
