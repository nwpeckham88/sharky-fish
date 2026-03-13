#!/bin/sh
set -e

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

echo "sharky-fish: configuring user sharky (${PUID}:${PGID})"

# Adjust the sharky group/user to match requested IDs.
groupmod -o -g "$PGID" sharky 2>/dev/null || true
usermod -o -u "$PUID" -g "$PGID" sharky 2>/dev/null || true

# If /dev/dri exists (GPU passthrough), add sharky to the render/video groups
# by reading the GIDs directly from the device files.
if [ -d /dev/dri ]; then
    for dev in /dev/dri/renderD*; do
        if [ -e "$dev" ]; then
            DEV_GID=$(stat -c '%g' "$dev")
            if ! getent group "$DEV_GID" > /dev/null 2>&1; then
                groupadd -g "$DEV_GID" hostrender 2>/dev/null || true
            fi
            GROUP_NAME=$(getent group "$DEV_GID" | cut -d: -f1)
            usermod -aG "$GROUP_NAME" sharky 2>/dev/null || true
            echo "sharky-fish: added sharky to group ${GROUP_NAME} (GID ${DEV_GID}) for $dev"
        fi
    done
fi

# Ensure runtime directories exist and are writable by the service user.
for dir in /config /data /ingest; do
    mkdir -p "$dir" 2>/dev/null || true
    chown sharky:sharky "$dir" 2>/dev/null || \
        echo "sharky-fish: warning - cannot chown $dir (check host volume ownership/permissions)"
done

# Drop privileges and run the application.
exec gosu sharky /usr/local/bin/sharky-fish "$@"
