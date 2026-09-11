#!/usr/bin/env bash
# Self-signed cert for local HTTPS: localhost, 127.0.0.1, and host IPv4 addresses.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
CRT="$DIR/localhost.crt"
KEY="$DIR/localhost.key"
if [[ -f "$CRT" && -f "$KEY" ]]; then
  echo "using existing $CRT"
  exit 0
fi
command -v openssl >/dev/null || {
  echo "openssl is required to generate deploy/certs/localhost.{crt,key}" >&2
  exit 1
}
SAN="DNS:localhost,IP:127.0.0.1"
if command -v hostname >/dev/null; then
  for ip in $(hostname -I 2>/dev/null || true); do
    if [[ "$ip" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ && "$ip" != 127.* ]]; then
      SAN="$SAN,IP:$ip"
    fi
  done
fi
openssl req -x509 -newkey rsa:2048 -sha256 -days 825 -nodes \
  -keyout "$KEY" -out "$CRT" \
  -subj "/CN=KnowledgeBrain local" \
  -addext "subjectAltName=$SAN"
chmod 644 "$CRT"
chmod 600 "$KEY"
echo "wrote $CRT ($SAN)"
