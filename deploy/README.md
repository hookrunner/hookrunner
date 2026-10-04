```sh
# ARM64 VPS, unprivileged user. Install Go 1.24+ and bubblewrap first.
go version
command -v bwrap

# Run from the repository.
mkdir -p "$HOME/.local/bin" "$HOME/.config/hookrunner"
(cd deploy/previewd && go build -o "$HOME/.local/bin/hookrunner-previewd" .)
cp deploy/preview.env.example "$HOME/.config/hookrunner/preview.env"
chmod 600 "$HOME/.config/hookrunner/preview.env"

# Generate PREVIEW_TOKEN, then set it, PREVIEW_IP and PREVIEW_REPOSITORY.
openssl rand -hex 32
${EDITOR:-vi} "$HOME/.config/hookrunner/preview.env"
# Private repo: also set PREVIEW_GITHUB_TOKEN with Contents: Read and Pull requests: Read.
# Allow inbound TCP 8080 and 20000-20100 in the VPS/cloud firewall.

# Start the daemon.
set -a
. "$HOME/.config/hookrunner/preview.env"
set +a
export PREVIEW_DATA="$HOME/.local/share/hookrunner-previews"
exec "$HOME/.local/bin/hookrunner-previewd"
```

```sh
# Another terminal with GitHub CLI. Replace OWNER/REPO and YOUR_VPS_IP.
gh auth login
gh variable set PREVIEW_API_URL --repo OWNER/REPO --body "http://YOUR_VPS_IP:8080"
# Paste the same PREVIEW_TOKEN as in the VPS environment file.
gh secret set PREVIEW_TOKEN --repo OWNER/REPO

# Deploy main once the workflow is on main.
gh workflow run main.yml --ref main --repo OWNER/REPO
# Play: http://YOUR_VPS_IP:20000/
```
