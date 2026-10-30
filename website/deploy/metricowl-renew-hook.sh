#!/bin/sh
set -eu
# Certbot passes this variable; never reload for another site's renewal.
if [ "${RENEWED_LINEAGE:-}" = /etc/letsencrypt/live/metricowl.cloud ]; then
    /usr/sbin/nginx -t
    /bin/systemctl reload nginx
fi
