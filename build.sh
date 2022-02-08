#!/bin/bash -e

printf 'Checking for autobuild... '
if ! command -v autobuild; then
    echo 'no'
    exit 2
else
    echo 'yes'
fi

cleanup () {
    echo "Cleaning up ..."
    rm -rf "$TMPDIR"
}

get_git_date() {
    local DATE
    DATE="$(git log -1 --format=%ct)"
    date --date "@$DATE" +%Y%m%d
}

printf 'Checking for build directory... '
OLD_WORKDIR="$PWD"
TMP_SPACE="$(df --sync /tmp/ --output=avail | tail -n1)"
if [[ "$TMP_SPACE" -gt 8388608 ]]; then
    TMPDIR="$(mktemp -d)"
else
    TMPDIR="$(mktemp -d -p /var/cache/)"
fi
echo "$TMPDIR"

trap cleanup EXIT SIGINT SIGTERM

cd "$TMPDIR"
echo "Start building ..."
git clone https://github.com/AOSC-Dev/repokit.git && cd repokit
cat << EOF >> autobuild/defines
# auto-generated ->
PKGVER=0+git$(get_git_date)
PKGREL=0
# <- auto-generated
EOF

autobuild

cp -v -- *.deb "$OLD_WORKDIR"/
rm -rv /debs
