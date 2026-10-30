# Metric Owl public website deployment

The public marketing website is isolated from the monitoring application's private
control center. Only `website/public/` is published. Application source, databases,
setup keys, SMTP credentials and verification artifacts must never be copied there.

## Current server layout

- Host: the operator's authorized Ubuntu server; site: `https://metricowl.cloud/`.
- Nginx: `/etc/nginx/sites-available/metricowl.cloud`, linked from `sites-enabled`.
- Releases: `/var/www/metricowl/releases/<release>/`.
- Active release: `/var/www/metricowl/current`, switched atomically.
- ACME validation directory: `/var/www/metricowl/acme/`.
- Certificate lineage: `/etc/letsencrypt/live/metricowl.cloud/` (apex + `www`).
- Guarded renewal hook: `/etc/letsencrypt/renewal-hooks/deploy/metricowl-reload.sh`.
- Separate access/error logs: `/var/log/nginx/metricowl-{access,error}.log`.

The existing Certbot timer renews this certificate. The hook only reloads Nginx
after a successful config check and only for this certificate lineage. HSTS applies
to the exact named hosts; no includeSubDomains or preload policy is set.

HTTP and HTTPS `www` redirect to the canonical HTTPS apex. There is no upstream
proxy and no public administrator/API endpoint. Existing sites and the loopback
monitoring listener are not modified.

Unknown and blocked paths keep HTTP 404 while rendering the public `404.html`
through an internal Nginx error-page route. Direct requests to `/404.html` also
remain 404. This error page must be included in every release.

## Publish a reviewed static release

From the repository root, using an authorized SSH identity:

```sh
python3 website/deploy/publish.py --host root@YOUR_SERVER \
  --control /path/to/existing/ssh-control-socket > artifacts/validation/website/release.json
```

Omit `--control` when normal key-based SSH authentication is configured. The script
requires all product screenshots/documentation, rejects symlinks, hidden files,
unexpected extensions and oversized files, and archives only `website/public/`.
It records the old/new release and SHA-256 of every public file. Never publish
the containing repository, a `.env` file or the artifacts directory.

The content switch needs no Nginx reload. Verify HTTPS, all links/assets, mobile and
desktop views, screenshot tabs/dialog, FAQ, and that `/admin`, `/api/v1/auth/status`,
`/.git/config`, `/.env` and arbitrary unknown paths return 404.

## Rollback

Read `previous_release` from the saved release record and verify it is an existing
directory under `/var/www/metricowl/releases/`. On the server:

```sh
ln -s /var/www/metricowl/releases/PREVIOUS_RELEASE /var/www/metricowl/current.rollback
mv -Tf /var/www/metricowl/current.rollback /var/www/metricowl/current
```

No application data is affected. For a vhost change, first restore only this site's
previous config, then run `nginx -t && systemctl reload nginx`. Do not restart Nginx
or change unrelated hosts. Keep the prior release until post-deploy checks pass.

## Read-only baseline and certificate test

```sh
ssh root@YOUR_SERVER python3 - < website/deploy/snapshot.py > before.json
ssh root@YOUR_SERVER 'certbot renew --cert-name metricowl.cloud --dry-run --run-deploy-hooks'
```

The inventory deliberately excludes Metric Owl's new configuration and all private
key material. It compares pre-existing Nginx/renewal/public-certificate hashes,
existing HTTP/HTTPS response status, and the Nginx master PID. Existing Nginx
protocol-option redefinition warnings predate this site and are outside this change.
