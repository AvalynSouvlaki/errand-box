#!/bin/sh
# Turns a Debian 13 system container into the errand-box guest image.
# `errand-box image build` pushes this file and its inputs to /run/eb-build
# in a fresh images:debian/13 instance and runs it as root there. It also
# runs unchanged in a distrobuilder build, from /opt/eb-build, because
# distrobuilder mounts a fresh tmpfs on /run while an action runs.
#
# Inputs, in the directory that holds this file:
#   launch, guest-init   the files of this directory
#   kage.tar.xz          the kage release archive, digest already checked
#   proxy-ca.crt         optional: the CA of the host's egress proxy
# Environment:
#   EB_PROXY             optional http://host:port of the egress proxy relay
#   EB_REGISTRY_MIRROR   optional Docker Hub mirror (edge case E-16)
#   EB_GUEST_PROXY_PORT  port where guest services reach the broker (3128)
# Exit: 0 built, 1 a step failed.
set -eu
in=$(cd "$(dirname "$0")" && pwd)
port=${EB_GUEST_PROXY_PORT:-3128}
say() { printf 'provision: %s\n' "$*"; }
fail() { printf 'provision: FAILED: %s\n' "$*" >&2; exit 1; }

say "apt over https"
for f in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
  [ -f "$f" ] && sed -i 's#http://deb.debian.org#https://deb.debian.org#g; s#http://security.debian.org#https://security.debian.org#g' "$f"
done
build_conf=/etc/apt/apt.conf.d/99errand-box-build
if [ -n "${EB_PROXY:-}" ]; then
  {
    printf 'Acquire::https::Proxy "%s";\n' "$EB_PROXY"
    printf 'Acquire::http::Proxy "%s";\n' "$EB_PROXY"
    if [ -f "$in/proxy-ca.crt" ]; then
      # The proxy's CA joins the public roots instead of replacing them: a
      # proxy that passes some connections through without re-signing them
      # would otherwise fail those (E-38).
      {
        [ -f /etc/ssl/certs/ca-certificates.crt ] && cat /etc/ssl/certs/ca-certificates.crt
        cat "$in/proxy-ca.crt"
      } > "$in/ca-bundle.crt"
      printf 'Acquire::https::CaInfo "%s/ca-bundle.crt";\n' "$in"
    fi
  } > "$build_conf"
fi

say "apt-get update"
apt-get -qq update || fail "apt-get update"
# apt-get update exits 0 even when every fetch failed (edge case E-14), so
# check that the main archive is really there.
apt-cache policy | grep -q 'deb.debian.org/debian trixie/main' || fail "apt-get update reached no Debian archive; check the proxy"

say "packages"
export DEBIAN_FRONTEND=noninteractive
apt-get -qq install -y --no-install-recommends \
  ca-certificates curl wget git openssh-client gnupg sudo procps psmisc iproute2 util-linux \
  less vim-tiny nano jq ripgrep fd-find tree file unzip zip xz-utils zstd bzip2 tmux strace \
  build-essential pkg-config python3 python3-venv python3-pip pipx nodejs npm \
  docker.io docker-cli docker-buildx docker-compose iptables nftables \
  podman crun uidmap fuse-overlayfs slirp4netns passt fuse3 rclone >/dev/null || fail "apt-get install"

say "the agent account (uid 1000)"
if ! id agent >/dev/null 2>&1; then
  getent passwd 1000 >/dev/null && fail "uid 1000 already exists: $(getent passwd 1000)"
  groupadd --gid 1000 agent
  useradd --uid 1000 --gid 1000 --create-home --shell /bin/bash agent
fi
usermod -aG docker agent
printf 'agent ALL=(ALL) NOPASSWD:ALL\n' > /etc/sudoers.d/agent
chmod 0440 /etc/sudoers.d/agent
# The guest has 65536 ids (the restricted project forbids a larger
# security.idmap.size). Rootless podman gets the ids above the agent's own
# (E-30); an image file owned by an id above 64535 then fails to unpack
# rootless, and `sudo podman` runs it instead.
for f in /etc/subuid /etc/subgid; do
  grep -v '^agent:' "$f" 2>/dev/null > "$f.eb" || true
  printf 'agent:1001:64535\n' >> "$f.eb"
  mv "$f.eb" "$f"
  chmod 0644 "$f"
done
# pasta, podman's default for rootless, copies the routes of the guest, and
# the guest has no default route, so it fails ("Couldn't set IPv4 route(s)
# in guest"). slirp4netns needs none; its connections leave from the guest,
# so they reach the broker on 169.254.169.1 (E-30).
mkdir -p /etc/containers/containers.conf.d
printf '[network]\ndefault_rootless_network_cmd = "slirp4netns"\n' > /etc/containers/containers.conf.d/40-errand-box-rootless.conf

say "launcher and boot step"
install -d -m 0755 /usr/local/libexec/errand-box
install -m 0755 "$in/launch" /usr/local/libexec/errand-box/launch
install -m 0755 "$in/guest-init" /usr/local/libexec/errand-box/guest-init
printf 'd /run/errand-box 0700 agent agent -\n' > /usr/lib/tmpfiles.d/errand-box.conf
cat > /etc/systemd/system/errand-box-guest.service <<'EOF'
[Unit]
Description=errand-box guest boot step (broker address, cgroup v1 podman settings)
DefaultDependencies=no
After=local-fs.target systemd-tmpfiles-setup.service
Before=docker.service podman.service containerd.service sysinit.target

[Service]
Type=oneshot
ExecStart=/usr/local/libexec/errand-box/guest-init
RemainAfterExit=yes

[Install]
WantedBy=sysinit.target
EOF
systemctl enable errand-box-guest.service >/dev/null 2>&1 || ln -sf /etc/systemd/system/errand-box-guest.service /etc/systemd/system/sysinit.target.wants/errand-box-guest.service

say "system services reach the broker on 169.254.169.1:$port"
proxy="http://169.254.169.1:$port"
cat > /etc/apt/apt.conf.d/80errand-box-proxy <<EOF
Acquire::http::Proxy "$proxy";
Acquire::https::Proxy "$proxy";
EOF
for svc in docker containerd; do
  mkdir -p "/etc/systemd/system/$svc.service.d"
  cat > "/etc/systemd/system/$svc.service.d/errand-box-proxy.conf" <<EOF
[Service]
Environment=HTTP_PROXY=$proxy HTTPS_PROXY=$proxy NO_PROXY=localhost,127.0.0.1,169.254.169.1
EOF
done
if [ -n "${EB_REGISTRY_MIRROR:-}" ]; then
  mkdir -p /etc/docker
  printf '{\n  "registry-mirrors": ["%s"]\n}\n' "$EB_REGISTRY_MIRROR" > /etc/docker/daemon.json
fi

say "units that wait for a network the guest does not have"
for u in systemd-networkd-wait-online.service apt-daily.timer apt-daily-upgrade.timer man-db.timer e2scrub_all.timer fstrim.timer; do
  systemctl mask "$u" >/dev/null 2>&1 || true
done
mkdir -p /etc/systemd/journald.conf.d
printf '[Journal]\nSystemMaxUse=200M\n' > /etc/systemd/journald.conf.d/errand-box.conf

say "kage"
tmp=$(mktemp -d)
tar -xJf "$in/kage.tar.xz" -C "$tmp"
k=$(find "$tmp" -type f -name kage -perm -u+x | head -n 1)
[ -n "$k" ] || fail "no kage binary in the archive"
install -m 0755 "$k" /usr/local/bin/kage
rm -rf "$tmp"
/usr/local/bin/kage --version >/dev/null 2>&1 || fail "kage does not run in the guest"

say "clean up"
rm -f "$build_conf"
apt-get -qq clean
rm -rf /var/lib/apt/lists/* /var/cache/apt/*.bin /tmp/* /var/tmp/*
printf 'errand-box guest\nbuilt %s\nkage %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$(/usr/local/bin/kage --version 2>/dev/null | head -n 1)" > /etc/errand-box-guest
say "done"
