#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 2 ]]; then
    echo 'Usage: bash tools/publish-aur.sh AUR_ARCHIVE VERSION' >&2
    exit 2
fi
: "${AUR_USERNAME:?Configure the AUR_USERNAME secret}"
: "${AUR_EMAIL:?Configure the AUR_EMAIL secret}"
: "${AUR_SSH_PRIVATE_KEY:?Configure the AUR_SSH_PRIVATE_KEY secret}"
archive=$(realpath -- "$1")
version=$2
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
umask 077
printf '%s\n' "$AUR_SSH_PRIVATE_KEY" > "$work/key"
ssh-keyscan -t ed25519 aur.archlinux.org > "$work/known_hosts"
export GIT_SSH_COMMAND="ssh -i '$work/key' -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile='$work/known_hosts'"
tar -xzf "$archive" -C "$work"
git clone ssh://aur@aur.archlinux.org/bo-music.git "$work/repository"
cp "$work/bo-music/PKGBUILD" "$work/bo-music/.SRCINFO" "$work/repository/"
cd "$work/repository"
git config user.name "$AUR_USERNAME"
git config user.email "$AUR_EMAIL"
git add PKGBUILD .SRCINFO
if git diff --cached --quiet; then
    echo 'AUR metadata is already up to date.'
    exit 0
fi
git commit -m "Update bo-music to $version"
git push origin HEAD:master
