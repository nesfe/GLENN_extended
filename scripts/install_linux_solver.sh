#!/usr/bin/env bash
# Install the pinned OpenCFD solver on a Debian/Ubuntu build worker so that
# bundle_linux.py can relocate it into the GLENN package. Pinned by checksum:
# a silently newer solver would change released numbers without a code change.
set -euo pipefail

repository=https://dl.openfoam.com/repos/deb/dists/jammy/main/pool/2412_260127
solver=openfoam2412_2412.260127-1_amd64.deb
common=openfoam2412-common_2412.260127-1_all.deb
checksums="12fab3754b9ae5e2fb19edadf31e4b5e3995c85ac8f8c867bb6064bc2b7993cd  $solver
956359cdbfd0e3a75ce1dd05e522ddfa43a124c6a087722169f275a5d40d238f  $common"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL --retry 3 "$repository/binary-amd64/$solver" -o "$work/$solver"
curl -fsSL --retry 3 "$repository/binary-all/$common" -o "$work/$common"
(cd "$work" && printf '%s\n' "$checksums" | sha256sum --check --strict)

sudo apt-get update
# apt resolves the solver's runtime dependencies from the distribution itself.
sudo apt-get install -y "$work/$common" "$work/$solver"
test -x /usr/lib/openfoam/openfoam2412/platforms/linux64GccDPInt32Opt/bin/simpleFoam
